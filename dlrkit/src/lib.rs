use core::ffi::{c_int, c_void};

use std::{
    error::Error,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;

// TODO: Windows support, see GetModuleHandleExW:
// https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw
#[cfg(unix)]
pub unsafe fn sym_by_addr(addr: usize) -> Result<SymInfo, Box<dyn Error>> {
    unsafe { unix::sym_by_addr(addr) }
}

pub struct SymInfo {
    pub object_name: String,
    pub object_base_addr: *const c_void,
    pub sym_name: String,
    pub sym_addr: *const c_void,
}

impl std::fmt::Display for SymInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "object_name: '{}' | ", self.object_name)?;

        write!(
            f,
            "object_base_addr: 0x{:x?} | ",
            self.object_base_addr as usize
        )?;

        write!(f, "sym_name: '{}' | ", self.sym_name)?;

        write!(f, "sym_addr: 0x{:x?}", self.sym_addr as usize)?;

        Ok(())
    }
}

pub enum OpenMode {
    Unix(c_int),
    Win32(u32),
}

impl std::fmt::Display for OpenMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenMode::Unix(_) => write!(f, "unix")?,
            OpenMode::Win32(_) => write!(f, "windows")?,
        }

        Ok(())
    }
}

pub struct Dl {
    hnd: *mut c_void,
}

unsafe impl Send for Dl {}
unsafe impl Sync for Dl {}

impl Dl {
    pub unsafe fn open(file: Option<&str>) -> Result<Self, Box<dyn Error>> {
        #[cfg(unix)]
        unsafe {
            Dl::open_mode(file, OpenMode::Unix(unix::RTLD_NOW))
        }

        #[cfg(windows)]
        unsafe {
            Dl::open_mode(file, OpenMode::Win32(0))
        }
    }

    pub unsafe fn open_mode(file: Option<&str>, mode: OpenMode) -> Result<Self, Box<dyn Error>> {
        let result;

        #[cfg(unix)]
        unsafe {
            result = unix::do_dlopen(file, mode);
        }

        #[cfg(windows)]
        unsafe {
            result = windows::load_library_exw(file, core::ptr::null_mut(), mode)
        }

        Ok(Self { hnd: result? })
    }

    pub unsafe fn handle(&self) -> *mut c_void {
        self.hnd
    }

    pub unsafe fn sym<T>(&self, symbol_name: &str) -> Result<Sym<T>, Box<dyn Error>> {
        // This check comes from dlopen2. It ensures that T is
        // the same size as a pointer.
        //
        // Copyright (c) 2017 Szymon Wieloch
        // Copyright (C) 2019 Ahmed Masud <ahmed.masud@saf.ai>
        // Copyright (C) 2022 OpenByte <development.openbyte@gmail.com>
        if size_of::<T>() != size_of::<*mut ()>() {
            panic!("type T has a different size than a pointer");
        }

        let result;

        #[cfg(unix)]
        unsafe {
            result = unix::do_dlsym(self.hnd, symbol_name);
        }

        #[cfg(windows)]
        unsafe {
            result = windows::get_proc_address(self.hnd, symbol_name);
        }

        let sym_ptr = result?;

        // Based on work by Chayim Friedman:
        // https://stackoverflow.com/a/71373744
        let sym_transmute = unsafe { std::mem::transmute_copy(&sym_ptr) };

        Ok(Sym::new(sym_transmute, sym_ptr as usize))
    }

    pub unsafe fn close(self) -> Result<(), Box<dyn Error>> {
        #[cfg(unix)]
        unsafe {
            unix::do_dlclose(self.hnd)
        }

        #[cfg(windows)]
        unsafe {
            windows::free_library(self.hnd)
        }
    }
}

/// Safe wrapper around a symbol obtained from `Dl`.
///
/// This is the most generic type, valid for obtaining functions,
/// references and pointers. It does not accept null value of
/// the library symbol. Other types may provide more specialized
/// functionality better for some use cases.
///
/// This originally appeared in the dlopen2 Rust library, maintained
/// by OpenByteDev.
///
/// Copyright (c) 2017 Szymon Wieloch
/// Copyright (C) 2019 Ahmed Masud <ahmed.masud@saf.ai>
/// Copyright (C) 2022 OpenByte <development.openbyte@gmail.com>
#[derive(Debug, Clone, Copy)]
pub struct Sym<'lib, T: 'lib> {
    symbol: T,
    addr: usize,
    pd: PhantomData<&'lib T>,
}

impl<'lib, T> Sym<'lib, T> {
    pub fn new(symbol: T, addr: usize) -> Sym<'lib, T> {
        Sym {
            symbol,
            addr,
            pd: PhantomData,
        }
    }

    pub fn addr(&self) -> usize {
        self.addr
    }
}

impl<'lib, T> Deref for Sym<'lib, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.symbol
    }
}

impl<'lib, T> DerefMut for Sym<'lib, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.symbol
    }
}

unsafe impl<'lib, T: Send> Send for Sym<'lib, T> {}
unsafe impl<'lib, T: Sync> Sync for Sym<'lib, T> {}
