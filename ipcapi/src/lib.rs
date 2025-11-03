use core::ffi::c_int;

use std::{error::Error, io};

const TYPE_SIZE: usize = 1;
const C_INT_SIZE: usize = std::mem::size_of::<c_int>();

pub enum Request {
    Socket(SocketRequest),
    Connect(ConnectRequest),
}

impl Request {
    pub fn next<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut msg_type_raw: [u8; 1] = [0];

        reader
            .read_exact(&mut msg_type_raw)
            .map_err(|err| format!("failed to read request header - {err}"))?;

        let msg_type = RequestType::from_u8(msg_type_raw[0])?;

        match msg_type {
            RequestType::Socket => Ok(Self::Socket(SocketRequest::next(reader)?)),
            RequestType::Connect => Ok(Self::Connect(ConnectRequest::next(reader)?)),
        }
    }
}

pub enum RequestType {
    Socket,
    Connect,
}

impl RequestType {
    pub fn from_u8(msg_type: u8) -> Result<Self, Box<dyn Error>> {
        match msg_type {
            1 => Ok(RequestType::Socket),
            2 => Ok(RequestType::Connect),
            _ => Err(format!("unknown request type: {msg_type:x?}"))?,
        }
    }
}

pub struct SocketRequest {
    pub domain: c_int,
    pub stype: c_int,
    pub protocol: c_int,
}

impl SocketRequest {
    const PAYLOAD_SIZE: usize = C_INT_SIZE * 3;
    const FULL_MSG_SIZE: usize = Self::PAYLOAD_SIZE + TYPE_SIZE;

    const DOMAIN_START: usize = 0;
    const DOMAIN_END: usize = Self::DOMAIN_START + C_INT_SIZE;

    const STYPE_START: usize = Self::DOMAIN_END;
    const STYPE_END: usize = Self::STYPE_START + C_INT_SIZE;

    const PROTOCOL_START: usize = Self::STYPE_END;
    const PROTOCOL_END: usize = Self::PROTOCOL_START + C_INT_SIZE;

    pub fn next<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut payload = [0u8; Self::PAYLOAD_SIZE];

        reader
            .read_exact(&mut payload)
            .map_err(|err| format!("failed to read socket request payload - {err}"))?;

        let mut domain_raw = [0u8; C_INT_SIZE];
        domain_raw.copy_from_slice(&payload[Self::DOMAIN_START..Self::DOMAIN_END]);

        let mut stype_raw = [0u8; C_INT_SIZE];
        stype_raw.copy_from_slice(&payload[Self::STYPE_START..Self::STYPE_END]);

        let mut protocol_raw = [0u8; C_INT_SIZE];
        protocol_raw.copy_from_slice(&payload[Self::PROTOCOL_START..Self::PROTOCOL_END]);

        let domain = c_int::from_le_bytes(domain_raw);

        if domain == ctypes::AF_UNIX {
            return Err("AF_UNIX sockets not permitted")?;
        }

        Ok(Self {
            domain: domain,
            stype: c_int::from_le_bytes(stype_raw),
            protocol: c_int::from_le_bytes(protocol_raw),
        })
    }

    pub fn bytes(&self) -> [u8; Self::FULL_MSG_SIZE] {
        let mut b = [0u8; Self::FULL_MSG_SIZE];

        b[0] = 1;

        b[Self::DOMAIN_START + TYPE_SIZE..Self::DOMAIN_END + TYPE_SIZE]
            .copy_from_slice(&c_int::to_le_bytes(self.domain));

        b[Self::STYPE_START + TYPE_SIZE..Self::STYPE_END + TYPE_SIZE]
            .copy_from_slice(&c_int::to_le_bytes(self.stype));

        b[Self::PROTOCOL_START + TYPE_SIZE..Self::PROTOCOL_END + TYPE_SIZE]
            .copy_from_slice(&c_int::to_le_bytes(self.protocol));

        b
    }
}

pub struct ConnectRequest {
    pub socket_fd: c_int,
    pub name: ctypes::sockaddr,
    pub namelen: ctypes::socklen_t,
}

impl ConnectRequest {
    const FD_SIZE: usize = std::mem::size_of::<c_int>();
    const NAME_SIZE: usize = std::mem::size_of::<ctypes::sockaddr>();
    const NAMELEN_SIZE: usize = std::mem::size_of::<ctypes::socklen_t>();

    const PAYLOAD_SIZE: usize = Self::FD_SIZE + Self::NAME_SIZE + Self::NAMELEN_SIZE;
    const FULL_MSG_SIZE: usize = Self::PAYLOAD_SIZE + TYPE_SIZE;

    const FD_START: usize = 0;
    const FD_END: usize = Self::FD_START + Self::FD_SIZE;

    const NAME_START: usize = Self::FD_END;
    const NAME_END: usize = Self::NAME_START + Self::NAME_SIZE;

    const NAMELEN_START: usize = Self::NAME_END;
    const NAMELEN_END: usize = Self::NAMELEN_START + Self::NAMELEN_SIZE;

    pub fn next<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut payload = [0u8; Self::PAYLOAD_SIZE];

        reader
            .read_exact(&mut payload)
            .map_err(|err| format!("failed to read connect request payload - {err}"))?;

        let mut fd_raw = [0; Self::FD_SIZE];
        fd_raw.copy_from_slice(&payload[Self::FD_START..Self::FD_END]);

        let mut name_raw = [0; Self::NAME_SIZE];
        name_raw.copy_from_slice(&payload[Self::NAME_START..Self::NAME_END]);

        let mut namelen_raw = [0; Self::NAMELEN_SIZE];
        namelen_raw.copy_from_slice(&payload[Self::NAMELEN_START..Self::NAMELEN_END]);

        Ok(Self {
            socket_fd: c_int::from_le_bytes(fd_raw),
            name: ctypes::sockaddr::from_bytes(&name_raw),
            namelen: ctypes::socklen_t::from_be_bytes(namelen_raw),
        })
    }

    pub fn bytes(&self) -> [u8; Self::FULL_MSG_SIZE] {
        let mut b = [0u8; Self::FULL_MSG_SIZE];

        b[0] = 2;

        b[Self::FD_START + TYPE_SIZE..Self::FD_END + TYPE_SIZE]
            .copy_from_slice(&c_int::to_le_bytes(self.socket_fd));

        b[Self::NAME_START + TYPE_SIZE..Self::NAME_END + TYPE_SIZE]
            .copy_from_slice(&self.name.bytes());

        b[Self::NAMELEN_START + TYPE_SIZE..Self::NAMELEN_END + TYPE_SIZE]
            .copy_from_slice(&ctypes::socklen_t::to_be_bytes(self.namelen));

        b
    }
}

pub enum Response {
    Success,
    FailureInt(c_int),
    Failure(String),
}

// pub enum ResponseType {
//     Success,
//     FailureInt,
//     Failure,
// }

// impl ResponseType {
//     pub fn from_u8
// }

impl Response {
    pub fn next<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut msg_type_raw: [u8; 1] = [0];

        reader.read_exact(&mut msg_type_raw)?;

        let msg_type = msg_type_raw[0];

        match msg_type {
            1 => Ok(Self::Success),
            2 => Self::read_error_msg(reader),
            3 => Self::read_failure_int(reader),
            _ => Err(format!("unknown response type: {msg_type:x}"))?,
        }
    }

    fn read_error_msg<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut msg_len_raw: [u8; 2] = [0, 0];

        reader
            .read_exact(&mut msg_len_raw)
            .map_err(|err| format!("failed to read error response length - {err}"))?;

        let len = u16::from_le_bytes(msg_len_raw) as usize;

        if len == 0 {
            return Err("failure message did not provide any error details")?;
        }

        let mut err_string = Vec::with_capacity(len);

        reader
            .read_exact(&mut err_string)
            .map_err(|err| format!("failed to read error response payload - {err}"))?;

        Ok(Self::Failure(String::from_utf8(err_string)?))
    }

    fn read_failure_int<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut int_raw = [0u8; C_INT_SIZE];

        reader
            .read_exact(&mut int_raw)
            .map_err(|err| format!("failed to read failure int response payload - {err}"))?;

        Ok(Self::FailureInt(c_int::from_le_bytes(int_raw)))
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut buf = Vec::<u8>::new();

        match self {
            Self::Success => buf.push(1),
            Self::Failure(err) => {
                buf.push(2);

                let err_bytes = err.as_bytes();

                let err_len = err_bytes.len().to_le_bytes();

                buf.extend_from_slice(&err_len);

                buf.extend_from_slice(err_bytes);
            }
            Self::FailureInt(i) => {
                buf.push(3);

                buf.extend_from_slice(&i.to_le_bytes());
            }
        }

        buf
    }
}
