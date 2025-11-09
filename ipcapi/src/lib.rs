use core::ffi::c_int;

use std::{
    error::Error,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
};

const TYPE_SIZE: usize = 1;
const C_INT_SIZE: usize = std::mem::size_of::<c_int>();

pub enum Request {
    Connect(ConnectRequest),
}

impl Request {
    pub fn next<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut msg_type_raw = [0u8; 1];

        reader
            .read_exact(&mut msg_type_raw)
            .map_err(|err| format!("failed to read request header - {err}"))?;

        let msg_type = RequestType::from_u8(msg_type_raw[0])?;

        match msg_type {
            RequestType::Connect => Ok(Self::Connect(ConnectRequest::next(reader)?)),
        }
    }
}

pub enum RequestType {
    Connect,
}

impl RequestType {
    pub fn from_u8(msg_type: u8) -> Result<Self, Box<dyn Error>> {
        match msg_type {
            2 => Ok(RequestType::Connect),
            _ => Err(format!("unknown request type: {msg_type:x?}"))?,
        }
    }
}

pub struct ConnectRequest {
    pub sa: std::net::SocketAddr,
}

impl ConnectRequest {
    const AF_SIZE: usize = std::mem::size_of::<c_int>();
    const PORT_SIZE: usize = std::mem::size_of::<u16>();
    const ADDR_SIZE: usize = std::mem::size_of::<u128>();
    const FLOWLABEL_SIZE: usize = std::mem::size_of::<u32>();
    const SCOPEID_SIZE: usize = std::mem::size_of::<u32>();

    const PAYLOAD_SIZE: usize = Self::AF_SIZE
        + Self::PORT_SIZE
        + Self::ADDR_SIZE
        + Self::FLOWLABEL_SIZE
        + Self::SCOPEID_SIZE;
    const FULL_MSG_SIZE: usize = Self::PAYLOAD_SIZE + TYPE_SIZE;

    const AF_START: usize = 0;
    const AF_END: usize = Self::AF_START + Self::AF_SIZE;

    const PORT_START: usize = Self::AF_END;
    const PORT_END: usize = Self::PORT_START + Self::PORT_SIZE;

    const ADDR_START: usize = Self::PORT_END;
    const ADDR_END: usize = Self::ADDR_START + Self::ADDR_SIZE;

    const FLOWLABEL_START: usize = Self::ADDR_END;
    const FLOWLABEL_END: usize = Self::FLOWLABEL_START + Self::FLOWLABEL_SIZE;

    const SCOPEID_START: usize = Self::FLOWLABEL_END;
    const SCOPEID_END: usize = Self::SCOPEID_START + Self::SCOPEID_SIZE;

    pub fn next<R: io::Read>(mut reader: R) -> Result<Self, Box<dyn Error>> {
        let mut payload = [0u8; Self::PAYLOAD_SIZE];

        reader
            .read_exact(&mut payload)
            .map_err(|err| format!("failed to read connect request payload - {err}"))?;

        let address_family = c_int::from_le_bytes(
            payload[Self::AF_START..Self::AF_END]
                .try_into()
                .map_err(|err| format!("failed to parse address family bits - {err}"))?,
        );

        let port = u16::from_le_bytes(
            payload[Self::PORT_START..Self::PORT_END]
                .try_into()
                .map_err(|err| format!("failed to parse port bits - {err}"))?,
        );

        let addr = u128::from_le_bytes(
            payload[Self::ADDR_START..Self::ADDR_END]
                .try_into()
                .map_err(|err| format!("failed to parse addr bits - {err}"))?,
        );

        let socketaddr: SocketAddr = match address_family {
            ctypes::AF_INET => {
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from_bits(addr as u32), port))
            }
            ctypes::AF_INET6 => {
                let flowlabel = u32::from_le_bytes(
                    payload[Self::FLOWLABEL_START..Self::FLOWLABEL_END]
                        .try_into()
                        .map_err(|err| format!("failed to parse flowlabel bits - {err}"))?,
                );

                let scopeid = u32::from_le_bytes(
                    payload[Self::SCOPEID_START..Self::SCOPEID_END]
                        .try_into()
                        .map_err(|err| format!("failed to parse scope id bits - {err}"))?,
                );

                SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from_bits(addr),
                    port,
                    flowlabel,
                    scopeid,
                ))
            }
            _ => return Err(format!("unsupported address family: {address_family}"))?,
        };

        Ok(Self { sa: socketaddr })
    }

    pub fn bytes(&self) -> [u8; Self::FULL_MSG_SIZE] {
        let mut b = [0u8; Self::FULL_MSG_SIZE];

        b[0] = 2;

        let mut flowlabel: u32 = 0;
        let mut scopeid: u32 = 0;

        let af_and_addr: (c_int, u128) = match self.sa {
            SocketAddr::V4(v4) => (ctypes::AF_INET, v4.ip().to_bits() as u128),
            SocketAddr::V6(v6) => {
                flowlabel = v6.flowinfo();
                scopeid = v6.scope_id();

                (ctypes::AF_INET6, v6.ip().to_bits())
            }
        };

        b[Self::AF_START + TYPE_SIZE..Self::AF_END + TYPE_SIZE]
            .copy_from_slice(&c_int::to_le_bytes(af_and_addr.0));

        b[Self::PORT_START + TYPE_SIZE..Self::PORT_END + TYPE_SIZE]
            .copy_from_slice(&u16::to_le_bytes(self.sa.port()));

        b[Self::ADDR_START + TYPE_SIZE..Self::ADDR_END + TYPE_SIZE]
            .copy_from_slice(&u128::to_le_bytes(af_and_addr.1));

        b[Self::FLOWLABEL_START + TYPE_SIZE..Self::FLOWLABEL_END + TYPE_SIZE]
            .copy_from_slice(&u32::to_le_bytes(flowlabel));

        b[Self::SCOPEID_START + TYPE_SIZE..Self::SCOPEID_END + TYPE_SIZE]
            .copy_from_slice(&u32::to_le_bytes(scopeid));

        b
    }
}

pub enum Response {
    Success,
    FailureInt(c_int),
    Failure(String),
}

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
