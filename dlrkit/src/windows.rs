use core::ffi::{c_char, c_void};

use std::{error::Error, ffi::CString};

use crate::OpenMode;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(
        lp_lib_file_name: *const u16,
        hfile: *mut c_void,
        dwflags: u32,
    ) -> *mut c_void;

    fn GetProcAddress(hmodule: *mut c_void, lp_proc_name: *const c_char) -> *mut c_void;

    fn FreeLibrary(hlibmodule: *mut c_void) -> bool;
}

pub unsafe fn load_library_exw(
    lp_lib_file_name: Option<&str>,
    hfile: *mut c_void,
    mode: OpenMode,
) -> Result<*mut c_void, Box<dyn Error>> {
    if lp_lib_file_name.is_none() {
        return Err("lp_lib_file_name is none")?;
    }

    let dwflags: u32 = match mode {
        OpenMode::Win32(v) => v,
        _ => return Err(format!("unknown open mode: {mode}").into()),
    };

    let lp_lib_file_name = lp_lib_file_name.unwrap();

    let mut lp_lib_file_name_utf16 = lp_lib_file_name.encode_utf16().collect::<Vec<_>>();
    lp_lib_file_name_utf16.push(0);

    let result = LoadLibraryExW(lp_lib_file_name_utf16.as_ptr(), hfile, dwflags);
    if result.is_null() {
        return Err(format!(
            "load library failed - {}",
            std::io::Error::last_os_error()
        ))?;
    }

    Ok(result)
}

pub unsafe fn get_proc_address(
    hmodule: *mut c_void,
    lp_proc_name: &str,
) -> Result<*mut c_void, Box<dyn Error>> {
    let lp_proc_name = CString::new(lp_proc_name)?;

    let result = GetProcAddress(hmodule, lp_proc_name.as_ptr());
    if result.is_null() {
        return Err(format!(
            "get proc address failed - {}",
            std::io::Error::last_os_error()
        ))?;
    }

    Ok(result)
}

pub unsafe fn free_library(hmodule: *mut c_void) -> Result<(), Box<dyn Error>> {
    if !FreeLibrary(hmodule) {
        return Err(format!(
            "failed to free library - {}",
            std::io::Error::last_os_error()
        ))?;
    }

    Ok(())
}
