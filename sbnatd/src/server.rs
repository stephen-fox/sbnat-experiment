#![allow(non_camel_case_types)]

use core::ffi::{c_int, c_void};

use std::{
    error::Error,
    fs,
    io::Write,
    os::{
        fd::{AsRawFd, RawFd},
        unix::{
            fs::PermissionsExt,
            net::{SocketAddr, UnixListener, UnixStream},
        },
    },
    path::PathBuf,
};

use ipcapi::{ConnectRequest, Request, Response};
use passfd::FdPassingExt;

pub struct Config {
    pub listen_path: PathBuf,
}

pub struct Server {
    config: Config,
    listener: UnixListener,
}

impl Server {
    pub fn block_and_serve(config: Config) -> Result<(), Box<dyn Error>> {
        let mut server = Self::listen(config)?;

        loop {
            let mut client = server.accept()?;

            if let Err(err) = client.handle_one_request() {
                eprintln!("failed to handle client request - {err}");
            }
        }
    }

    pub fn listen(config: Config) -> Result<Server, Box<dyn Error>> {
        let _ = fs::remove_file(&config.listen_path);

        let listener = UnixListener::bind(&config.listen_path)
            .map_err(|err| format!("failed to create unix socket - {err}"))?;

        let listener_perms = fs::metadata(&config.listen_path)
            .map_err(|err| format!("failed to get unix socket permissions - {err}"))?;

        let mut listener_perms = listener_perms.permissions();

        listener_perms.set_mode(0o666);

        fs::set_permissions(&config.listen_path, listener_perms)
            .map_err(|err| format!("failed to update unix socket permissions - {err}"))?;

        let cloned_listener = listener
            .try_clone()
            .map_err(|err| format!("failed to clone unix socket fd - {err}"))?;

        Ok(Self {
            config: config,
            listener: cloned_listener,
        })
    }

    pub fn accept(&mut self) -> Result<Conn, Box<dyn Error>> {
        let result = self
            .listener
            .accept()
            .map_err(|err| format!("failed to accept new client connection - {err}"))?;

        Ok(Conn::new(result.0, result.1))
    }

    pub fn shutdown(&mut self) -> Result<(), std::io::Error> {
        let err = do_shutdown(self.listener.as_raw_fd(), ShutdownHow::SHUT_RD);

        let _ = fs::remove_file(&self.config.listen_path);

        err
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

enum ShutdownHow {
    SHUT_RD,
    _SHUT_WR,
    _SHUT_RDWR,
}

impl ShutdownHow {
    fn to_c_type(&self) -> core::ffi::c_int {
        match self {
            ShutdownHow::SHUT_RD => 0,
            ShutdownHow::_SHUT_WR => 1,
            ShutdownHow::_SHUT_RDWR => 2,
        }
    }
}

fn do_shutdown(socket: RawFd, how: ShutdownHow) -> Result<(), std::io::Error> {
    let result = unsafe { shutdown(socket, how.to_c_type()) };
    if result == 0 {
        return Ok(());
    }

    Err(std::io::Error::last_os_error())
}

unsafe extern "C" {
    fn shutdown(socket_fd: core::ffi::c_int, how: core::ffi::c_int) -> core::ffi::c_int;
}

pub struct Conn {
    socket: UnixStream,
    _addr: SocketAddr,
}

impl Conn {
    pub fn new(socket: UnixStream, addr: SocketAddr) -> Self {
        Self {
            socket: socket,
            _addr: addr,
        }
    }

    pub fn handle_one_request(&mut self) -> Result<(), Box<dyn Error>> {
        match Request::next(&self.socket)? {
            Request::Connect(r) => {
                eprintln!("handle_connect_request start");
                let result = self.handle_connect_request(r);
                eprintln!("handle_connect_request end");
                result
            }
        }
    }

    fn handle_connect_request(&mut self, req: ConnectRequest) -> Result<(), Box<dyn Error>> {
        let socket_fd_from_client = self
            .socket
            .recv_fd()
            .map_err(|err| format!("connect: failed to receive socket fd from client - {err}"))?;

        let socket_info_result = get_socket_info(socket_fd_from_client);

        unsafe { close(socket_fd_from_client) };

        let socket_info = match socket_info_result {
            Ok(info) => info,
            Err(err) => {
                self.socket
                    .write_all(&Response::Failure(err.to_string()).bytes())
                    .map_err(|err| {
                        format!("connect: failed to write get_socket_info error to client - {err}")
                    })?;

                return Err(format!(
                    "connect: failed to get info for client socket - {err}"
                ))?;
            }
        };

        #[cfg(feature = "debug")]
        eprintln!(
            "connect: socket info - domain: {} | type: {} | proto: {}",
            socket_info.domain, socket_info.stype, socket_info.protocol
        );

        if socket_info.domain == ctypes::AF_UNIX {
            self.socket
                .write_all(&Response::FailureInt(-1).bytes())
                .map_err(|err| {
                    format!("connect: failed to write get_socket_info error to client - {err}")
                })?;

            return Err("connect: client sent a AF_UNIX socket")?;
        }

        // We need to override socket(2) because FreeBSD seems to know
        // if a socket originated from a jail. Thus, the client needs
        // the daemon to create the socket.
        //
        // For example, calling connect(2) with a socket fd created in
        // a vnet jail will result in this error:
        //
        //   connect failed -1 - Network is unreachable (os error 51)
        let new_socket_fd =
            unsafe { socket(socket_info.domain, socket_info.stype, socket_info.protocol) };

        if new_socket_fd < 0 {
            let last_err_i = std::io::Error::last_os_error().raw_os_error().unwrap_or(-1);

            self.socket
                .write_all(&Response::FailureInt(last_err_i).bytes())
                .map_err(|err| {
                    format!("connect: failed to write socket error response to client - {err}")
                })?;

            return Ok(());
        }

        let sockaddr_data = match req.sa {
            std::net::SocketAddr::V4(v4) => {
                let tmp = ctypes::sockaddr_in {
                    sin_len: 0,
                    sin_family: ctypes::AF_INET as u8,
                    sin_port: req.sa.port(),
                    sin_addr: ctypes::in_addr {
                        s_addr: v4.ip().to_bits(),
                    },
                    sin_zero: [0u8; 8],
                };

                (
                    (&raw const tmp) as *const ctypes::sockaddr,
                    std::mem::size_of::<ctypes::sockaddr_in>(),
                )
            }
            std::net::SocketAddr::V6(v6) => {
                let tmp = ctypes::sockaddr_in6 {
                    sin6_len: 0,
                    sin6_family: ctypes::AF_INET6 as u8,
                    sin6_port: req.sa.port(),
                    sin6_addr: ctypes::in6_addr {
                        s6_addr: v6.ip().octets(),
                    },
                    sin6_flowinfo: v6.flowinfo(),
                    sin6_scope_id: v6.scope_id(),
                };

                (
                    (&raw const tmp) as *const ctypes::sockaddr,
                    std::mem::size_of::<ctypes::sockaddr_in6>(),
                )
            }
        };

        let connect_result = unsafe {
            connect(
                new_socket_fd,
                sockaddr_data.0,
                sockaddr_data.1 as ctypes::socklen_t,
            )
        };

        if connect_result != 0 {
            unsafe { close(new_socket_fd) };

            let last_err_i = std::io::Error::last_os_error().raw_os_error().unwrap_or(-1);

            self.socket
                .write_all(&Response::FailureInt(last_err_i).bytes())
                .map_err(|err| {
                    format!("failed to write connect error response to client - {err}")
                })?;

            return Ok(());
        };

        if let Err(err) = self.socket.write_all(&Response::Success.bytes()) {
            unsafe { close(new_socket_fd) };

            return Err(format!(
                "connect: failed to write success response to client - {err}"
            ))?;
        }

        let result = self.socket.send_fd(new_socket_fd);

        unsafe { close(new_socket_fd) };

        match result {
            Ok(()) => Ok(()),
            Err(err) => Err(format!(
                "connect: failed to send new connected socket fd to client - {err}"
            ))?,
        }
    }
}

fn get_socket_info(socket_fd: c_int) -> Result<SocketInfo, GetSocketInfoError> {
    let mut result: c_int;

    let mut domain: c_int = 0;
    let mut stype: c_int = 0;
    let mut protocol: c_int = 0;

    let mut optlen = std::mem::size_of::<c_int>() as ctypes::socklen_t;

    result = unsafe {
        getsockopt(
            socket_fd,
            ctypes::SOL_SOCKET,
            ctypes::SO_DOMAIN,
            (&raw mut domain) as *mut _,
            &mut optlen,
        )
    };
    if result != 0 {
        return Err(GetSocketInfoError::DomainFailed(
            std::io::Error::last_os_error(),
        ));
    }

    let mut optlen = std::mem::size_of::<c_int>() as ctypes::socklen_t;

    result = unsafe {
        getsockopt(
            socket_fd,
            ctypes::SOL_SOCKET,
            ctypes::SO_TYPE,
            (&raw mut stype) as *mut _,
            &mut optlen,
        )
    };
    if result != 0 {
        return Err(GetSocketInfoError::SocketTypeFailed(
            std::io::Error::last_os_error(),
        ));
    }

    let mut optlen = std::mem::size_of::<c_int>() as ctypes::socklen_t;

    result = unsafe {
        getsockopt(
            socket_fd,
            ctypes::SOL_SOCKET,
            ctypes::SO_PROTOCOL,
            (&raw mut protocol) as *mut _,
            &mut optlen,
        )
    };
    if result != 0 {
        return Err(GetSocketInfoError::ProtocolFailed(
            std::io::Error::last_os_error(),
        ));
    }

    Ok(SocketInfo {
        domain: domain,
        stype: stype,
        protocol: protocol,
    })
}

struct SocketInfo {
    domain: c_int,
    stype: c_int,
    protocol: c_int,
}

enum GetSocketInfoError {
    DomainFailed(std::io::Error),
    SocketTypeFailed(std::io::Error),
    ProtocolFailed(std::io::Error),
}

impl std::fmt::Display for GetSocketInfoError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::DomainFailed(err) => write!(f, "failed to get socket domain - {err}"),
            Self::SocketTypeFailed(err) => write!(f, "failed to get socket type - {err}"),
            Self::ProtocolFailed(err) => {
                write!(f, "failed to get socket protocol- {err}")
            }
        }
    }
}

unsafe extern "C" {
    fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int;

    fn getsockopt(
        sockfd: c_int,
        level: c_int,
        optname: c_int,
        optval: *mut c_void,
        optlen: *mut ctypes::socklen_t,
    ) -> c_int;

    fn connect(
        socket_fd: c_int,
        name: *const ctypes::sockaddr,
        namelen: ctypes::socklen_t,
    ) -> c_int;

    fn close(fd: c_int) -> c_int;
}
