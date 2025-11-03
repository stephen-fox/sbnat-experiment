#![allow(non_camel_case_types)]

use core::ffi::c_int;

use std::{
    error::Error,
    fs,
    io::Write,
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::{SocketAddr, UnixListener, UnixStream},
    },
    path::PathBuf,
};

use ipcapi::{ConnectRequest, Request, Response, SocketRequest};
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

        let listener = UnixListener::bind(&config.listen_path)?;

        let cloned_listener = listener.try_clone()?;

        Ok(Self {
            config: config,
            listener: cloned_listener,
        })
    }

    pub fn accept(&mut self) -> Result<Conn, Box<dyn Error>> {
        let result = self.listener.accept()?;

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
            Request::Socket(r) => self.handle_socket_request(r),
            Request::Connect(r) => self.handle_connect_request(r),
        }
    }

    fn handle_socket_request(&mut self, r: SocketRequest) -> Result<(), Box<dyn Error>> {
        let res = unsafe { socket(r.domain, r.stype, r.protocol) };

        if res < 0 {
            // TODO: close socket.

            self.socket
                .write_all(&Response::FailureInt(res).bytes())
                .map_err(|err| {
                    format!("socket: failed to write socket result to client - {err}")
                })?;

            return Ok(());
        }

        self.socket
            .write_all(&Response::Success.bytes())
            .map_err(|err| {
                format!("socket: failed to write success response to client - {err} ")
            })?;

        self.socket
            .send_fd(res)
            .map_err(|err| format!("socket: failed to send socket fd to client - {err} "))?;

        eprintln!("handle_socket_request done");

        Ok(())
    }

    fn handle_connect_request(&mut self, r: ConnectRequest) -> Result<(), Box<dyn Error>> {
        let socket = self
            .socket
            .recv_fd()
            .map_err(|err| format!("connect: failed to receive socket fd from client - {err}"))?;

        let res = unsafe { connect(socket, &r.name, r.namelen) };

        if res == 0 {
            self.socket
                .write_all(&Response::Success.bytes())
                .map_err(|err| {
                    format!("connect: failed to write success response to client - {err}")
                })?
        } else {
            self.socket
                .write_all(&Response::FailureInt(res).bytes())
                .map_err(|err| format!("failed to write error response to client - {err}"))?
        };

        eprintln!("handle_connect_request done");

        Ok(())
    }
}

unsafe extern "C" {
    fn socket(domain: c_int, socket_type: c_int, protocol: c_int) -> c_int;

    fn connect(
        socket_fd: c_int,
        name: *const ctypes::sockaddr,
        namelen: ctypes::socklen_t,
    ) -> c_int;
}
