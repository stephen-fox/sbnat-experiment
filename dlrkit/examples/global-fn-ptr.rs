use std::sync::OnceLock;

const LIBRARY_PATH: &str = {
    if cfg!(target_os = "windows") {
        "target\\debug\\deps\\example_lib.dll"
    } else if cfg!(any(target_os = "macos", target_os = "ios")) {
        "target/debug/deps/libexample_lib.dylib"
    } else {
        "target/debug/deps/libexample_lib.so"
    }
};

static LIB: OnceLock<dlrkit::Dl> = OnceLock::new();

static ADD: OnceLock<dlrkit::Sym<AddSignature>> = OnceLock::new();

type AddSignature = fn(left: u64, right: u64) -> u64;

fn main() {
    do_add();
}

fn do_add() {
    let add = ADD.get_or_init(|| load_add());

    eprintln!("add result: {}", add(67, 2));
}

fn load_add() -> dlrkit::Sym<'static, AddSignature> {
    let lib = LIB.get_or_init(|| load_library());

    unsafe { lib.sym("add").unwrap() }
}

fn load_library() -> dlrkit::Dl {
    unsafe { dlrkit::Dl::open(Some(LIBRARY_PATH)).unwrap() }
}
