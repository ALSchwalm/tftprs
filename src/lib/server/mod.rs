use std::net::{UdpSocket, SocketAddr};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread;
use std::str;
use std::fs::File;
use std::io::{Read, ErrorKind, Write};

use packet::Packet;
use packet::data::TftpData;
use packet::data;
use packet::ack::TftpAck;
use packet::error::TftpError;
use codes::{ErrorCode, TransferMode, Opcode};

pub struct TftpServer {
    socket: UdpSocket,
    root: PathBuf,
}

type PacketBuff = [u8; 1024];
impl TftpServer {

    /// Create a TFTP server that serves from the given path. This server
    /// will not respond to requests until `start` is called.
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

    /// Start the server. Requests will be handled in different threads.
    pub fn start(&self) -> ! {
        loop {
            let mut packet_buffer = [0u8; 1024];
            let (count, addr) = self.socket.recv_from(&mut packet_buffer).unwrap();
            self.handle_request(addr, packet_buffer, count);
        }
    }

    // Dispatch an incoming request to the appropriate handler. Does nothing
    // if the packet is ill-formed or unexpected.
    fn handle_request(&self, addr: SocketAddr, packet: PacketBuff, length: usize) {
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

    // Extract opcode from a given packet. Returns an Err if the packet is too small
    // or not on of the specified opcodes.
    fn get_packet_opcode(&self, length: usize, packet: &PacketBuff) -> Result<Opcode, TftpError> {
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

    // Translates a standard rust io error into a TftpError to be sent
    // over the wire.
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

    // Extract the transfer mode and path from the given packet
    fn parse_rw_request(packet: &mut PacketBuff, length: usize) -> Result<(&str, TransferMode), TftpError> {
        let packet = &mut packet[2..length];
        let mut parts = packet.splitn_mut(3, |x| *x == 0);
        let filename = str::from_utf8(parts.next().unwrap()).unwrap();
        let mode_str = str::from_utf8(parts.next().unwrap()).unwrap();
        let mode = match &mode_str.to_lowercase()[..] {
            "netascii" => TransferMode::NetAscii,
            "octet" => TransferMode::Octet,
            "email" => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Email transfer mode is not supported")
            }),
            _ => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Unknown transfer mode")
            }),
        };
        Ok((filename, mode))
    }

    fn handle_write_request(&self, addr: SocketAddr, packet: PacketBuff, length: usize) {
        let root = self.root.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let (filename, mode) = match Self::parse_rw_request(&mut inner_packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    socket.send_to(&e.as_packet(), addr);
                    return ();
                }
            };

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

        let mut file = match File::create(path) {
            Ok(f) => f,
            Err(e) => return Err(Self::translate_io_error(e.kind()))
        };

        // The buffer to receive data into. Max size is 512 payload bytes plus
        // 2 for opcode and 2 for
        let mut resp_buffer = [0u8; data::MAX_DATA_SIZE + 2 + 2];
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
                } else {

                    // This is the expected packet, so write it out
                    file.write(&data.data);
                    if data.data.len() < 512 {
                        let ack = TftpAck{number: i+1};
                        socket.send_to(&ack.as_packet(), addr).unwrap();

                        // No further packets, so stop
                        return Ok(());
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    fn handle_read_request(&self, addr: SocketAddr, packet: PacketBuff, length: usize) {
        let root = self.root.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let (filename, mode) = match Self::parse_rw_request(&mut inner_packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    socket.send_to(&e.as_packet(), addr);
                    return ();
                }
            };

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

        // We just need a 4 byte buffer for the ACK
        //FIXME: this allows larger messages that happen to start
        //       with the right bytes to be accepted
        let mut resp_buffer = [0u8; 4];
        let mut previous_bytes_sent = 0;

        for i in 1.. {
            let mut data_packet = TftpData{
                number: i,
                data: vec![0u8; data::MAX_DATA_SIZE]
            };

            let file_bytes = match file.read(&mut data_packet.data) {
                Ok(b) => b,
                Err(e) => return Err(Self::translate_io_error(e.kind()))
            };

            // If this is the end of the file (we've read less than 512 bytes),
            // then truncate the data vector so the packet won't be padded with
            // zeros
            if file_bytes < data::MAX_DATA_SIZE {
                data_packet.data.truncate(file_bytes);
            }

            // A 0 byte file should still get a response, so make sure
            // that we've sent one. Also, if the file length was a multiple
            // of 512, we need to send a 0 size response to show the end.
            // Otherwise, we're done
            if file_bytes == 0 && i > 1 && previous_bytes_sent < data::MAX_DATA_SIZE {
                return Ok(());
            } else {
                previous_bytes_sent = file_bytes;
                let expected_ack = TftpAck{number:i};

                // Loop until we receive an ACK from the appropriate source
                loop {
                    socket.send_to(&data_packet.as_packet(), addr).unwrap();
                    let (count, resp_addr) = socket.recv_from(&mut resp_buffer).unwrap();

                    let actual_ack = match TftpAck::from_buffer(&resp_buffer[..count]) {
                        Some(a) => a,
                        None => continue
                    };

                    // Receiving a packet from unexpected source does not
                    // interrupt the operation with the current client
                    if resp_addr != addr {
                        socket.send_to(&TftpError{
                            code: ErrorCode::UnknownTransferID,
                            message: None
                        }.as_packet(), resp_addr);
                    } else if expected_ack == actual_ack {
                        // The fragment has been sent and acknowledged
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}
