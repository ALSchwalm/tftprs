use std::net::{UdpSocket, ToSocketAddrs, SocketAddr};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread;
use std::str;
use std::fs::File;
use std::io::Read;

enum TransferMode {
    NetAscii,
    Octet, // 'email' is unsupported
}

enum Opcode {
    ReadRequest,
    WriteRequest,
    Data,
    Acknowledgment,
    Error,
}

enum ErrorCode {
    Undefined = 0,
    FileNotFound,
    AccessViolation,
    DiskFull,
    IllegalOperation,
    UnknownTransferID,
    FileExists,
    NoSuchUser,

    // Internal error codes
    SilentError,
}

struct TftpData {
    number: u16,
    data: [u8; 512],
    used: usize
}

impl TftpData {
    fn new() -> TftpData {
        TftpData {
            number : 0,
            data : [0u8; 512],
            used: 0
        }
    }

    fn as_packet(&self) -> Vec<u8> {
        let mut packet = vec![0u8, 3u8];
        let high = (self.number >> 8) as u8;
        let low = self.number as u8;

        packet.push(high);
        packet.push(low);
        packet.extend(self.data.iter().take(self.used));
        packet
    }
}

struct TftpError {
    code: ErrorCode,
    message: Option<&'static str>,
}

impl TftpError {
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
}

pub struct TftpServer {
    socket: UdpSocket,
    root: PathBuf,
}


impl TftpServer {
    pub fn new<S: AsRef<OsStr> + ?Sized>(root: &S) -> TftpServer {
        let socket = UdpSocket::bind("0.0.0.0:69").unwrap();
        TftpServer {
            socket: socket,
            root: PathBuf::from(root),
        }
    }

    pub fn start(&self) {
        loop {
            let mut packet_buffer = [0u8; 1024];
            let (count, addr) = self.socket.recv_from(&mut packet_buffer).unwrap();
            self.handle_request(addr, packet_buffer, count);
        }
    }

    fn handle_request(&self, addr: SocketAddr, packet: [u8; 1024], length: usize) {
        let code = self.get_packet_opcode(length, &packet);
        match code {
            Ok(code) => {
                match code {
                    Opcode::ReadRequest => self.handle_read_request(addr, packet, length),
                    _ => (),
                }
            }
            Err(_) => (),
        }
    }

    fn handle_read_request(&self, addr: SocketAddr, packet: [u8; 1024], length: usize) {
        let root = self.root.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;

            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(_) => panic!("Unable to bind to port")
            };

            let packet = &mut inner_packet[2..length];
            let mut parts = packet.splitn_mut(3, |x| *x == 0);

            let filename = str::from_utf8(parts.next().unwrap()).unwrap();
            let _ = str::from_utf8(parts.next().unwrap()).unwrap();

            let full_path = root.join(filename);
            if !full_path.exists() {
                let error_packet = TftpError{
                    code: ErrorCode::FileNotFound,
                    message: None
                }.as_packet();

                socket.send_to(&error_packet, addr);
                return;
            }

            let mut data_packet = TftpData::new();
            let mut file = File::open(full_path).unwrap();
            for i in 1.. {
                data_packet.number = i;
                let bytes = file.read(&mut data_packet.data).unwrap();
                if bytes == 0 {
                    return;
                } else {
                    data_packet.used = bytes;
                    socket.send_to(&data_packet.as_packet(), addr);
                }
            }
        });
    }

    fn get_packet_opcode(&self, length: usize, packet: &[u8; 1024]) -> Result<Opcode, TftpError> {
        // Must be at least two bytes for the opcode
        if length < 2 || packet[0] != 0 {
            return Err(TftpError {
                code: ErrorCode::SilentError,
                message: None,
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
                    code: ErrorCode::SilentError,
                    message: None,
                })
            }
        }
    }
}
