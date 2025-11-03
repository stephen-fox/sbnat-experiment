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
    let lib = unsafe { dlrkit::Dl::open(Some(LIBRARY_PATH))? };

    let add = unsafe { lib.sym::<fn(left: u64, right: u64) -> u64>("add")? };

    eprintln!("add result: {}", add(62, 7));

    unsafe { lib.close()? };

    Ok(())
}
