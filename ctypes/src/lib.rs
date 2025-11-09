#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int};
use std::error::Error;

pub const AF_UNIX: c_int = 1;

pub const AF_INET: c_int = 2;

pub const AF_INET6: c_int = {
    if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        0x1a
    } else if cfg!(any(target_os = "openbsd", target_os = "netbsd")) {
        0x18
    } else if cfg!(target_os = "freebsd") {
        0x1c
    } else if cfg!(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos"
    )) {
        0x1e
    } else {
        // Linux-like.
        0x0a
    }
};

pub const SOCK_STREAM: c_int = 1;
pub const SOCK_CLOEXEC: c_int = 0x10000000;
pub const SUN_PATH_OFFSET: usize = std::mem::offset_of!(sockaddr_un, sun_path);

pub const SOL_SOCKET: c_int = {
    if cfg!(all(target_os = "linux", target_arch = "mips")) {
        0xffff
    } else if cfg!(all(target_os = "linux", target_arch = "sparc")) {
        0xffff
    } else if cfg!(target_os = "linux") {
        0x01
    } else if cfg!(target_os = "fuchsia") {
        0x01
    } else {
        // Unix-like systems.
        0xffff
    }
};

pub const SO_DOMAIN: c_int = {
    if cfg!(target_os = "android") {
        0x27
    } else if cfg!(all(target_os = "linux", target_arch = "mips")) {
        0x1029
    } else if cfg!(all(target_os = "linux", target_arch = "powerpc")) {
        0x27
    } else if cfg!(all(target_os = "linux", target_arch = "sparc")) {
        0x1029
    } else if cfg!(target_os = "linux") {
        0x27
    } else if cfg!(target_os = "fuchsia") {
        0x27
    } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        0x100c
    } else if cfg!(target_os = "openbsd") {
        0x1024
    } else {
        // Unix-like systems.
        0x1019
    }
};

pub const SO_TYPE: c_int = {
    if cfg!(target_os = "android") {
        0x3
    } else if cfg!(all(target_os = "linux", target_arch = "mips")) {
        0x1008
    } else if cfg!(all(target_os = "linux", target_arch = "sparc")) {
        0x1008
    } else if cfg!(target_os = "linux") {
        0x3
    } else if cfg!(target_os = "fuchsia") {
        0x3
    } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        0x1008
    } else {
        // Unix-like systems.
        0x1008
    }
};

pub const SO_PROTOCOL: c_int = {
    if cfg!(target_os = "android") {
        0x26
    } else if cfg!(all(target_os = "linux", target_arch = "mips")) {
        0x1028
    } else if cfg!(all(target_os = "linux", target_arch = "sparc")) {
        0x1028
    } else if cfg!(target_os = "linux") {
        0x26
    } else if cfg!(target_os = "fuchsia") {
        0x26
    } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        0x1009
    } else if cfg!(target_os = "openbsd") {
        0x1025
    } else {
        // Unix-like systems.
        0x1016
    }
};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct sockaddr {
    pub sa_len: u8,
    pub sa_family: sa_family_t,
    pub sa_data: [c_char; 14],
}

impl sockaddr {
    pub fn from_bytes(bytes: &[u8; std::mem::size_of::<sockaddr>()]) -> Self {
        let sa_len: u8 = bytes[0];

        let sa_family: sa_family_t = bytes[1];

        let mut sa_data: [c_char; 14] = [0; 14];

        sa_data.copy_from_slice(&bytes[2..]);

        Self {
            sa_len: sa_len,
            sa_family: sa_family,
            sa_data: sa_data,
        }
    }

    pub fn bytes(&self) -> [u8; std::mem::size_of::<sockaddr>()] {
        let mut b = [0u8; std::mem::size_of::<sockaddr>()];

        b[0] = self.sa_len;

        b[1] = self.sa_family;

        b[2..16].copy_from_slice(&self.sa_data);

        b
    }

    pub unsafe fn to_addr(ptr: *const Self) -> Result<std::net::SocketAddr, Box<dyn Error>> {
        let sa_family = unsafe { (*ptr).sa_family };

        match sa_family as c_int {
            AF_INET => {
                let in_ptr: *const sockaddr_in = unsafe { std::mem::transmute_copy(&ptr) };

                let port = unsafe { (*in_ptr).sin_port };

                let addr_raw = unsafe { (*in_ptr).sin_addr.s_addr };

                let addr = std::net::Ipv4Addr::from_bits(addr_raw);

                Ok(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
                    addr, port,
                )))
            }
            AF_INET6 => {
                let in_ptr: *const sockaddr_in6 = unsafe { std::mem::transmute_copy(&ptr) };

                let port = unsafe { (*in_ptr).sin6_port };

                let addr_raw = unsafe { (*in_ptr).sin6_addr.s6_addr };

                let addr = std::net::Ipv6Addr::from_bits(u128::from_le_bytes(addr_raw));

                let flowinfo = unsafe { (*in_ptr).sin6_flowinfo };

                let scope_id = unsafe { (*in_ptr).sin6_scope_id };

                Ok(std::net::SocketAddr::V6(std::net::SocketAddrV6::new(
                    addr, port, flowinfo, scope_id,
                )))
            }
            _ => Err(format!("unsupported socket address family: {sa_family}"))?,
        }
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct sockaddr_in {
    #[cfg(not(target_os = "linux"))]
    pub sin_len: u8,
    pub sin_family: sa_family_t,
    pub sin_port: in_port_t,
    pub sin_addr: in_addr,
    pub sin_zero: [c_char; 8],
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct sockaddr_in6 {
    pub sin6_len: u8,
    pub sin6_family: sa_family_t,
    pub sin6_port: in_port_t,
    pub sin6_flowinfo: u32,
    pub sin6_addr: in6_addr,
    pub sin6_scope_id: u32,
}

#[derive(Clone, Copy, Default)]
#[repr(align(4), C)]
pub struct in6_addr {
    pub s6_addr: [u8; 16],
}

pub type in_port_t = u16;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct in_addr {
    pub s_addr: in_addr_t,
}

pub type in_addr_t = u32;

#[repr(C)]
pub struct sockaddr_un {
    pub sun_len: u8,
    pub sun_family: sa_family_t,
    pub sun_path: [c_char; 104],
}

pub type sa_family_t = u8;

pub type socklen_t = u32;
