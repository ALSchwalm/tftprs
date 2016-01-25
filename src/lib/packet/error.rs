use std::io::ErrorKind;
use std::iter::FromIterator;
use std::slice::Iter;

use packet::Packet;
use codes::{ErrorCode, Opcode};


#[derive(Debug, PartialEq, Eq)]
pub struct TftpError {
    pub code: ErrorCode,
    pub message: Option<String>,
}

impl Packet for TftpError {
    fn as_packet(&self) -> Vec<u8> {
        // The Error Opcode, always 5
        let mut packet = vec![0u8, 5u8];
        let message = self.message.clone();
        let (code, message) = match self.code {
            ErrorCode::Undefined =>
                (0u8, message.unwrap_or("Undefined".to_string())),
            ErrorCode::FileNotFound =>
                (1u8, message.unwrap_or("File not found".to_string())),
            ErrorCode::AccessViolation =>
                (2u8, message.unwrap_or("Access violation".to_string())),
            ErrorCode::DiskFull =>
                (3u8, message.unwrap_or("Disk full".to_string())),
            ErrorCode::IllegalOperation =>
                (4u8, message.unwrap_or("Illegal operation".to_string())),
            ErrorCode::UnknownTransferID =>
                (5u8, message.unwrap_or("Unknown transfer id".to_string())),
            ErrorCode::FileExists =>
                (6u8, message.unwrap_or("File exists".to_string())),
            ErrorCode::NoSuchUser =>
                (7u8, message.unwrap_or("No such user".to_string())),
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
        if buf.len() <= 4 {
            return None
        } else if buf[0] != 0u8 || buf[1] != 5u8 {
            return None
        } else if buf[2] != 0u8 {
            return None
        }

        let code = match buf[3] {
            0u8 => ErrorCode::Undefined,
            1u8 => ErrorCode::FileNotFound,
            2u8 => ErrorCode::AccessViolation,
            3u8 => ErrorCode::DiskFull,
            4u8 => ErrorCode::IllegalOperation,
            5u8 => ErrorCode::UnknownTransferID,
            6u8 => ErrorCode::FileExists,
            7u8 => ErrorCode::NoSuchUser,
            _ => return None
        };

        let msg_buffer = buf[4..buf.len()-1].iter().cloned().collect::<Vec<_>>();
        let message = match String::from_utf8(msg_buffer) {
            Ok(s) => s,
            Err(_) => return None
        };

        Some(TftpError{
            code: code,
            message: Some(message)
        })
    }
}

#[test]
fn tftp_error_round_trip() {
    let error = TftpError{
        code: ErrorCode::Undefined,
        message: Some("Message".to_string())
    };
    let roundtrip = TftpError::from_buffer(&error.as_packet()).unwrap();

    assert_eq!(error, roundtrip);
}

#[test]
fn tftp_error_null_terminator() {
    let buffer = [0u8, 5u8, 0u8, 0u8];
    let packet = TftpError::from_buffer(&buffer);
    assert!(packet.is_none());

    let buffer = [0u8, 5u8, 0u8, 0u8, 0u8];
    let packet = TftpError::from_buffer(&buffer);
    assert!(packet.is_some());
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
            message: Some("An unknown IO error occurred".to_string())
        }
    }
}
