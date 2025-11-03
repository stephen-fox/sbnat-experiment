#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int};

pub const AF_UNIX: c_int = 1;
pub const SOCK_STREAM: c_int = 1;
pub const SOCK_CLOEXEC: c_int = 0x10000000;
pub const SUN_PATH_OFFSET: usize = std::mem::offset_of!(sockaddr_un, sun_path);

#[repr(C)]
pub struct sockaddr_un {
    pub sun_len: u8,
    pub sun_family: sa_family_t,
    pub sun_path: [c_char; 104],
}

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
}

pub type sa_family_t = u8;

pub type socklen_t = u32;
