use std::error::Error;

const LIBRARY_PATH: &str = {
    if cfg!(target_os = "windows") {
        "target\\debug\\deps\\example_lib.dll"
    } else if cfg!(any(target_os = "macos", target_os = "ios")) {
        "target/debug/deps/libexample_lib.dylib"
    } else {
        "target/debug/deps/libexample_lib.so"
    }
};

fn main() -> Result<(), Box<dyn Error>> {
    let _lib = unsafe { dlrkit::Dl::open(Some(LIBRARY_PATH))? };

    // TODO: Windows support.
    #[cfg(unix)]
    {
        let add_ptr = unsafe { _lib.sym::<*mut ()>("add")? };

        let info = unsafe { dlrkit::sym_by_addr(add_ptr.addr())? };

        eprintln!("{info}");

        unsafe { _lib.close()? };
    }

    Ok(())
}
