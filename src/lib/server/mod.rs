use std::net::{UdpSocket, SocketAddr};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread;
use std::str;
use std::fs::File;
use std::io::{Read, ErrorKind, Write};

#[derive(Debug)]
enum TransferMode {
    NetAscii,
    Octet,
    // 'email' is unsupported
}

#[derive(Debug)]
enum Opcode {
    ReadRequest,
    WriteRequest,
    Data,
    Acknowledgment,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
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

trait Packet: Sized {
    fn as_packet(&self) -> Vec<u8>;
    fn from_buffer(&[u8]) -> Option<Self>;
}

#[derive(Debug, PartialEq, Eq)]
struct TftpData {
    number: u16,
    data: Vec<u8>,
}

impl TftpData {
    fn new() -> TftpData {
        TftpData {
            number : 0,
            data : vec![0u8; 512],
        }
    }
}

impl Packet for TftpData {
    fn as_packet(&self) -> Vec<u8> {
        let high = (self.number >> 8) as u8;
        let low = self.number as u8;

        let mut packet = vec![0u8, 3u8, high, low];
        packet.extend(self.data.iter());
        packet
    }

    fn from_buffer(buf: &[u8]) -> Option<TftpData> {
        if buf.len() < 4 {
            return None
        } else if buf[0] != 0u8 || buf[1] != 3u8 {
            return None
        }
        Some(TftpData {
            number: ((buf[2] as u16) << 8) | buf[3] as u16,
            data: buf.to_vec()
        })
    }
}

#[test]
fn tftp_data_round_trip() {
    let data = TftpData::new();
    assert!(data == TftpData::from_buffer(&data.as_packet()).unwrap());
}

#[derive(Debug, PartialEq, Eq)]
struct TftpAck {
    number: u16
}

impl Packet for TftpAck {
    fn as_packet(&self) -> Vec<u8> {
        vec![
            0x00,
            0x04,
            (self.number >> 8) as u8,
            self.number as u8
        ]
    }

    fn from_buffer(buf: &[u8]) -> Option<TftpAck> {
        if buf.len() != 4 {
            return None
        } else if buf[0] != 0u8 || buf[1] != 4u8 {
            return None
        }
        Some(TftpAck{
            number: ((buf[2] as u16 ) << 8) | buf[3] as u16
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TftpError {
    code: ErrorCode,
    message: Option<&'static str>,
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

pub struct TftpServer {
    socket: UdpSocket,
    root: PathBuf,
}


impl TftpServer {
    pub fn new<S: AsRef<OsStr> + ?Sized>(root: &S) -> TftpServer {
        let socket = match UdpSocket::bind("0.0.0.0:69") {
            Ok(s) => s,
            Err(e) => panic!("bind:{}", e.to_string())
        };
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
                    Opcode::WriteRequest => self.handle_write_request(addr, packet, length),
                    _ => (),
                }
            }
            Err(_) => (),
        }
    }

    fn translate_io_error(e: ErrorKind) -> TftpError {
        return match e {
            ErrorKind::PermissionDenied => TftpError{
                code: ErrorCode::AccessViolation,
                message: None
            },
            ErrorKind::AlreadyExists => TftpError{
                code: ErrorCode::FileExists,
                message: None
            },
            _ => panic!("Unexpected io error:{:?}", e)
        }
    }

    fn parse_rw_request(packet: &mut [u8; 1024], length: usize) -> (&str, TransferMode) {
        let packet = &mut packet[2..length];
        let mut parts = packet.splitn_mut(3, |x| *x == 0);
        let filename = str::from_utf8(parts.next().unwrap()).unwrap();
        let mode_str = str::from_utf8(parts.next().unwrap()).unwrap();
        let mode = match &mode_str.to_lowercase()[..] {
            "netascii" => TransferMode::NetAscii,
            "octet" => TransferMode::Octet,
            "email" => panic!("Email mode is unsupported"),
            _ => panic!("Unknown mode")
        };
        (filename, mode)
    }

    fn handle_write_request(&self, addr: SocketAddr, packet: [u8; 1024], length: usize) {
        let root = self.root.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;
            let (filename, mode) = Self::parse_rw_request(&mut inner_packet, length);

            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let full_path = root.join(filename);
            match Self::recieve_file(&socket, &full_path, &mode, addr) {
                Ok(_) => (),

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {socket.send_to(&err.as_packet(), addr).unwrap();}
            }
        });
    }

    fn recieve_file(socket: &UdpSocket, path: &PathBuf,
                    mode: &TransferMode, addr: SocketAddr) -> Result<(), TftpError> {
        if path.exists() {
            return Err(TftpError{
                code: ErrorCode::FileExists,
                message: None
            });
        }

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return Err(Self::translate_io_error(e.kind()))
        };

        let mut resp_buffer = [0u8; 512 + 2 + 2];
        for i in 0.. {
            loop {
                let ack = TftpAck{number: i};
                socket.send_to(&ack.as_packet(), addr).unwrap();
                let (count, resp_addr) = socket.recv_from(&mut resp_buffer).unwrap();

                // Receiving a packet from unexpected source does not
                // interrupt the operation with the current client
                if resp_addr != addr {
                    socket.send_to(&TftpError{
                        code: ErrorCode::UnknownTransferID,
                        message: None
                    }.as_packet(), resp_addr);
                }

                let data =
                    match TftpData::from_buffer(&resp_buffer[..count]) {
                        Some(d) => d,
                        None => continue
                    };

                // This is an unexpected data packet (probably a retransmission)
                // so ack again
                if data.number != i+1 {
                    continue;
                }

                file.write(&data.data);
                break;
            }
        }

        Ok(())
    }

    fn handle_read_request(&self, addr: SocketAddr, packet: [u8; 1024], length: usize) {
        let root = self.root.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;
            let (filename, mode) = Self::parse_rw_request(&mut inner_packet, length);

            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let full_path = root.join(filename);

            match Self::send_file(&socket, &full_path, &mode, addr) {
                Ok(_) => (),

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {socket.send_to(&err.as_packet(), addr).unwrap();}
            }
        });
    }

    fn send_file(socket: &UdpSocket, path: &PathBuf,
                 mode: &TransferMode, addr: SocketAddr) -> Result<(), TftpError> {
        if !path.exists() {
            return Err(TftpError{
                code: ErrorCode::FileNotFound,
                message: None
            });
        }

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return Err(Self::translate_io_error(e.kind()))
        };

        let mut data_packet = TftpData::new();

        // We just need a 4 byte buffer for the ACK
        //FIXME: this allows larger messages that happen to start
        //       with the right bytes to be accepted
        let mut resp_buffer = [0u8; 4];

        let mut previous_bytes_sent = 0;
        for i in 1.. {
            data_packet.number = i;
            let file_bytes = match file.read(&mut data_packet.data) {
                Ok(b) => b,
                Err(e) => return Err(Self::translate_io_error(e.kind()))
            };

            // A 0 byte file should still get a response, so make sure
            // that we've sent one. Also, if the file length was a multiple
            // of 512, we need to send a 0 size response to show the end.
            // Otherwise, we're done
            if file_bytes == 0 && i > 1 && previous_bytes_sent < data_packet.data.len() {
                return Ok(());
            } else {
                previous_bytes_sent = file_bytes;

                // Loop until we receive an ACK from the appropriate source
                loop {
                    socket.send_to(&data_packet.as_packet(), addr);
                    let (count, resp_addr) = socket.recv_from(&mut resp_buffer).unwrap();

                    let expected = [
                        0u8,
                        4u8,
                        (data_packet.number >> 8) as u8,
                        data_packet.number as u8
                    ];

                    // Receiving a packet from unexpected source does not
                    // interrupt the operation with the current client
                    if resp_addr != addr {
                        socket.send_to(&TftpError{
                            code: ErrorCode::UnknownTransferID,
                            message: None
                        }.as_packet(), resp_addr);
                    } else if count == 4 && resp_buffer == expected {
                        // The fragment has been send and acknowledged
                        break;
                    }
                }
            }
        }
        Ok(())
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
