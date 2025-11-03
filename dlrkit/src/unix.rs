use core::{
    ffi::{c_char, c_int, c_void, CStr},
    ptr::{null, null_mut},
};

use std::{
    error::Error,
    ffi::CString,
    sync::{Mutex, OnceLock},
};

use crate::{OpenMode, SymInfo};

pub const RTLD_NOW: c_int = {
    if cfg!(all(target_os = "android", target_pointer_width = "32")) {
        0x00
    } else if cfg!(target_os = "haiku") {
        0x01
    } else {
        0x02
    }
};

// This constant's various values comes from
// the rust-lang/libc library.
pub const RTLD_DEFAULT: *mut c_void = {
    if cfg!(target_os = "fuchsia") {
        0i64 as *mut c_void
    } else if cfg!(target_os = "aix") {
        -1isize as *mut c_void
    } else if cfg!(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "ios",
        target_os = "macos",
        target_os = "tvos",
        target_os = "visionos",
    )) {
        -2isize as *mut c_void
    } else if cfg!(target_os = "haiku") {
        0isize as *mut c_void
    } else if cfg!(target_os = "hurd") {
        0i64 as *mut c_void
    } else if cfg!(all(target_os = "android", target_pointer_width = "32")) {
        -1isize as *mut c_void
    } else if cfg!(all(target_os = "android", target_pointer_width = "64")) {
        0i64 as *mut c_void
    } else if cfg!(target_os = "emscripten") {
        0i64 as *mut c_void
    } else if cfg!(target_os = "linux") {
        0i64 as *mut c_void
    } else if cfg!(target_os = "horizon") {
        0 as *mut c_void
    } else if cfg!(target_os = "rtems") {
        -2isize as *mut c_void
    } else if cfg!(target_os = "vita") {
        0 as *mut c_void
    } else if cfg!(target_os = "nto") {
        -2i64 as *mut c_void
    } else if cfg!(target_os = "nuttx") {
        0 as *mut c_void
    } else if cfg!(target_os = "redox") {
        0i64 as *mut c_void
    } else if cfg!(target_os = "solaris") {
        -2isize as *mut c_void
    } else if cfg!(target_os = "vxworks") {
        0i64 as *mut c_void
    } else {
        0x00 as *mut c_void
    }
};

extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;

    fn dlerror() -> *mut c_char;

    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;

    fn dladdr(addr: *const c_void, info: *mut c_void) -> c_int;

    fn dlclose(handle: *mut c_void) -> c_int;
}

pub struct DlInfo {
    pub dli_fname: *const c_char,
    pub dli_fbase: *mut c_void,
    pub dli_sname: *const c_char,
    pub dli_saddr: *mut c_void,
}

impl DlInfo {
    unsafe fn to_sym_info(&self) -> SymInfo {
        SymInfo {
            object_name: unsafe { const_c_char_to_string(self.dli_fname) },
            object_base_addr: self.dli_fbase,
            sym_name: unsafe { const_c_char_to_string(self.dli_sname) },
            sym_addr: self.dli_saddr,
        }
    }
}

unsafe fn const_c_char_to_string(p: *const c_char) -> String {
    if p.is_null() {
        return String::from("");
    }

    unsafe { CStr::from_ptr(p).to_string_lossy().to_string() }
}

pub unsafe fn do_dlopen(file: Option<&str>, mode: OpenMode) -> Result<*mut c_void, Box<dyn Error>> {
    let mode: c_int = match mode {
        OpenMode::Unix(v) => v,
        _ => return Err(format!("unsupported open mode type: {mode}").into()),
    };

    let handle = match file {
        Some(p) => {
            let path = CString::new(p)?;

            unsafe { dlopen(path.as_ptr(), mode) }
        }
        None => unsafe { dlopen(null_mut(), mode) },
    };

    if handle.is_null() {
        unsafe {
            match last_dlerror() {
                Some(err_msg) => return Err(err_msg)?,
                None => {
                    return Err("dlopen failed without any error (dlerror returned null)".into())
                }
            }
        }
    }

    Ok(handle)
}

pub unsafe fn do_dlsym(handle: *mut c_void, symbol: &str) -> Result<*mut c_void, Box<dyn Error>> {
    let name_cstr = CString::new(symbol)?;

    let symbol_ptr = unsafe { dlsym(handle, name_cstr.as_ptr()) };

    if symbol_ptr.is_null() {
        unsafe {
            match last_dlerror() {
                Some(err_msg) => return Err(err_msg)?,
                None => return Err("dlsym failed without any error (dlerror returned null)".into()),
            }
        }
    }

    Ok(symbol_ptr)
}

pub(crate) unsafe fn sym_by_addr(addr: usize) -> Result<SymInfo, Box<dyn Error>> {
    let dl_info = unsafe { do_dladdr(addr as *const c_void)? };

    Ok(unsafe { dl_info.to_sym_info() })
}

pub unsafe fn do_dladdr(addr: *const c_void) -> Result<DlInfo, Box<dyn Error>> {
    let mut info = DlInfo {
        dli_fname: null(),
        dli_fbase: null_mut(),
        dli_sname: null(),
        dli_saddr: null_mut(),
    };

    let info_ptr: *mut DlInfo = &mut info;

    // We could also use DlInfo in the function siganture,
    // but doing so produces a warning about unsafe FFI.
    // We avoid that warning by using *mut c_void as the
    // datatype and casting to *mut c_void here.
    //
    // Ref Alice's post here:
    // https://users.rust-lang.org/t/extern-block-uses-type-which-is-not-ffi-safe/
    let result = unsafe { dladdr(addr, info_ptr.cast()) };

    if result == 0 {
        unsafe {
            match last_dlerror() {
                Some(err_msg) => return Err(err_msg)?,
                None => {
                    return Err("dladdr failed without any error (dlerror returned null)".into())
                }
            }
        }
    }

    Ok(info)
}

pub unsafe fn do_dlclose(handle: *mut c_void) -> Result<(), Box<dyn Error>> {
    let result = unsafe { dlclose(handle) };

    if result != 0 {
        unsafe {
            match last_dlerror() {
                Some(err_msg) => return Err(err_msg)?,
                None => return Err("dlsym failed without any error (dlerror returned null)".into()),
            }
        }
    }

    Ok(())
}

static DLERROR_MUTEX: OnceLock<Mutex<u8>> = OnceLock::new();

unsafe fn last_dlerror() -> Option<String> {
    let mu = DLERROR_MUTEX.get_or_init(|| Mutex::new(0));

    let _mu = mu.lock().unwrap();

    let err_ptr = unsafe { dlerror() };
    if err_ptr.is_null() {
        return None;
    }

    Some(CStr::from_ptr(err_ptr).to_string_lossy().to_string())
}
