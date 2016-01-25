pub mod data;
pub mod ack;
pub mod error;

use packet::error::TftpError;
use codes::{Opcode, ErrorCode};

pub trait Packet: Sized {
    fn as_packet(&self) -> Vec<u8>;
    fn from_buffer(&[u8]) -> Option<Self>;
}

pub type PacketBuff = [u8; 1024];

// Extract opcode from a given packet. Returns an Err if the packet is too small
// or not on of the specified opcodes.
pub fn get_packet_opcode(length: usize, packet: &PacketBuff) -> Result<Opcode, TftpError> {
    // Must be at least two bytes for the opcode
    if length < 2 || packet[0] != 0 {
        return Err(TftpError {
            code: ErrorCode::Undefined,
            message: Some("Invalid opcode".to_string()),
        });
    }

    match packet[1] {
        1 => Ok(Opcode::ReadRequest),
        2 => Ok(Opcode::WriteRequest),
        3 => Ok(Opcode::Data),
        4 => Ok(Opcode::Acknowledgment),
        5 => Ok(Opcode::Error),
        _ => {
            Err(TftpError {
                code: ErrorCode::Undefined,
                message: Some("Invalid opcode".to_string()),
            })
        }
    }
}
