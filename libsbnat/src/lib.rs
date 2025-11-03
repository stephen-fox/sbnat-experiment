use core::ffi::c_int;

use std::{
    error::Error,
    io::Write,
    os::{
        fd::FromRawFd,
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::{Path, PathBuf},
    sync::OnceLock,
};

use ctor::ctor;
use ipcapi::{ConnectRequest, Response, SocketRequest};
use passfd::FdPassingExt;

const SOCKET_PATH: &str = "/sbnatd.sock";

static LIBC: OnceLock<dlrkit::Dl> = OnceLock::new();

static SOCKET: OnceLock<dlrkit::Sym<SocketFnSig>> = OnceLock::new();

static CONNECT: OnceLock<dlrkit::Sym<ConnectFnSig>> = OnceLock::new();

type SocketFnSig = fn(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int;

type ConnectFnSig =
    fn(socket_fd: c_int, sockaddr: *const ctypes::sockaddr, namelen: ctypes::socklen_t) -> c_int;

#[ctor]
fn on_load() {
    if let Err(err) = on_load_with_error() {
        panic!("libsbnat initialization failed - {err}");
    }
}

fn on_load_with_error() -> Result<(), Box<dyn Error>> {
    SOCKET.get_or_init(load_socket);

    CONNECT.get_or_init(load_connect);

    Ok(())
}

fn connect_to_daemon() -> Result<UnixStream, Box<dyn Error>> {
    let daemon_socket_path = match std::env::var("SBNAT_SOCKET_PATH") {
        Ok(path_str) => PathBuf::from(path_str),
        Err(_) => PathBuf::from(SOCKET_PATH),
    };

    connect_socket(&daemon_socket_path)
}

// Based on this example::
// https://docs-archive.freebsd.org/44doc/psd/20.ipctut/paper.pdf
fn connect_socket(path: &Path) -> Result<UnixStream, Box<dyn Error>> {
    let socket_fn = SOCKET.get_or_init(load_socket);
    let connect_fn = CONNECT.get_or_init(load_connect);

    let (addr, addr_len) = get_sockaddr_un(path)?;

    // socket(AF_UNIX, SOCK_STREAM, 0)
    let socket_fd = socket_fn(ctypes::AF_UNIX, ctypes::SOCK_STREAM, 0);
    if socket_fd == -1 {
        return Err(format!(
            "socket function failed - {}",
            std::io::Error::last_os_error()
        ))?;
    }

    // connect(sock, &server, sizeof(struct sockaddr_un)
    let result = connect_fn(socket_fd, (&raw const addr) as *const _, addr_len);
    if result == -1 {
        return Err(format!(
            "connect function failed - {}",
            std::io::Error::last_os_error()
        ))?;
    }

    Ok(unsafe { UnixStream::from_raw_fd(socket_fd) })
}

// Based on rust's std::os::unix::net::sockaddr_un:
// https://github.com/rust-lang/rust/blob/6380899f32599ea25615d4ccd708d0e8da652b0c/library/std/src/os/unix/net/addr.rs#L26
fn get_sockaddr_un(path: &Path) -> std::io::Result<(ctypes::sockaddr_un, ctypes::socklen_t)> {
    let mut addr: ctypes::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = ctypes::AF_UNIX as ctypes::sa_family_t;

    let bytes = path.as_os_str().as_bytes();

    if bytes.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "paths must not contain interior null bytes",
        ));
    }

    if bytes.len() >= addr.sun_path.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path must be shorter than SUN_LEN",
        ));
    }

    // SAFETY: `bytes` and `addr.sun_path` are not overlapping and
    // both point to valid memory.
    // NOTE: We zeroed the memory above, so the path is already null
    // terminated.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            addr.sun_path.as_mut_ptr().cast(),
            bytes.len(),
        )
    };

    let mut len = ctypes::SUN_PATH_OFFSET + bytes.len();

    match bytes.get(0) {
        Some(&0) | None => {}
        Some(_) => len += 1,
    }

    Ok((addr, len as ctypes::socklen_t))
}

fn load_socket() -> dlrkit::Sym<'static, SocketFnSig> {
    let lib = LIBC.get_or_init(load_library);

    unsafe { lib.sym("socket").unwrap() }
}

fn load_connect() -> dlrkit::Sym<'static, ConnectFnSig> {
    let lib = LIBC.get_or_init(load_library);

    unsafe { lib.sym("connect").unwrap() }
}

fn load_library() -> dlrkit::Dl {
    unsafe { dlrkit::Dl::open(Some("libc.so.7")).unwrap() }
}

#[unsafe(no_mangle)]
extern "C" fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int {
    let mut conn = match connect_to_daemon() {
        Ok(conn) => conn,
        Err(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] socket: failed to connect to daemon - {_err}");

            return -1;
        }
    };

    let req = SocketRequest {
        domain: domain,
        stype: socket_type,
        protocol: protocol,
    };

    if let Err(_err) = conn.write_all(&req.bytes()) {
        #[cfg(feature = "debug")]
        eprintln!("[libsbnat] socket: failed to send request - {_err}");

        return -1;
    }

    let resp = match Response::next(&conn) {
        Ok(r) => r,
        Err(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] socket: failed to receive or parse reponse - {_err}");

            return -1;
        }
    };

    match resp {
        Response::Success => {}
        Response::FailureInt(i) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] socket: remote socket call failed with: {i}");

            return i;
        }
        Response::Failure(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] socket: remote socket call failed with err: {_err}");

            return -1;
        }
    };

    match conn.recv_fd() {
        Ok(fd) => fd,
        Err(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] socket: failed to receive fd: {_err}");

            -1
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn connect(
    socket_fd: c_int,
    sockaddr: *const ctypes::sockaddr,
    namelen: ctypes::socklen_t,
) -> c_int {
    let mut conn = match connect_to_daemon() {
        Ok(conn) => conn,
        Err(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] connect: failed to connect to daemon - {_err}");

            return -1;
        }
    };

    let req = ConnectRequest {
        socket_fd: socket_fd,
        name: unsafe { *sockaddr },
        namelen: namelen,
    };

    if let Err(_err) = conn.write_all(&req.bytes()) {
        #[cfg(feature = "debug")]
        eprintln!("[libsbnat] connect: failed to send request - {_err}");

        return -1;
    }

    if let Err(_err) = conn.send_fd(socket_fd) {
        #[cfg(feature = "debug")]
        eprintln!("[libsbnat] connect: failed to send socket fd - {_err}");

        return -1;
    }

    let resp = match Response::next(&conn) {
        Ok(r) => r,
        Err(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] connect: failed to receive or parse reponse - {_err}");

            return -1;
        }
    };

    match resp {
        Response::Success => 0,
        Response::FailureInt(i) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] connect: remote connect call failed with: {i}");

            i
        }
        Response::Failure(_err) => {
            #[cfg(feature = "debug")]
            eprintln!("[libsbnat] connect: remote connect call failed with err: {_err}");

            -1
        }
    }
}
