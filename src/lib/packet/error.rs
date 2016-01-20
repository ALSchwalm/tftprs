use std::io::ErrorKind;

use packet::Packet;
use codes::{ErrorCode, Opcode};


#[derive(Debug, PartialEq, Eq)]
pub struct TftpError {
    pub code: ErrorCode,
    pub message: Option<&'static str>,
}

impl Packet for TftpError {
    fn as_packet(&self) -> Vec<u8> {
        // The Error Opcode, always 5
        let mut packet = vec![0u8, 5u8];

        let (code, message) = match self.code {
            ErrorCode::Undefined =>
                (0u8, self.message.unwrap_or("Undefined")),
            ErrorCode::FileNotFound =>
                (1u8, self.message.unwrap_or("File not found")),
            ErrorCode::AccessViolation =>
                (2u8, self.message.unwrap_or("Access violation")),
            ErrorCode::DiskFull =>
                (3u8, self.message.unwrap_or("Disk full")),
            ErrorCode::IllegalOperation =>
                (4u8, self.message.unwrap_or("Illegal operation")),
            ErrorCode::UnknownTransferID =>
                (5u8, self.message.unwrap_or("Unknown transfer id")),
            ErrorCode::FileExists =>
                (6u8, self.message.unwrap_or("File exists")),
            ErrorCode::NoSuchUser =>
                (7u8, self.message.unwrap_or("No such user")),
        };

        // The specific error code
        packet.push(0u8);
        packet.push(code);

        // Message and NULL terminator
        packet.extend(message.bytes());
        packet.push(0u8);

        packet
    }

    fn from_buffer(buf: &[u8]) -> Option<TftpError> {
        None
    }
}


// Translates a standard rust io error into a TftpError to be sent
// over the wire.
pub fn translate_io_error(e: ErrorKind) -> TftpError {
    return match e {
        ErrorKind::PermissionDenied => TftpError{
            code: ErrorCode::AccessViolation,
            message: None
        },
        ErrorKind::AlreadyExists => TftpError{
            code: ErrorCode::FileExists,
            message: None
        },
        ErrorKind::NotFound => TftpError {
            code: ErrorCode::FileNotFound,
            message: None
        },
        _ => TftpError {
            code: ErrorCode::Undefined,
            message: Some("An unknown IO error occurred")
        }
    }
}
