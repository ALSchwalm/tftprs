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
            _ => panic!("The given ErrorCode should not be sent in a packet")
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
