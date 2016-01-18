use std::net::{UdpSocket, SocketAddr, ToSocketAddrs};
use std::ffi::OsStr;
use std::path::{PathBuf, Path};
use std::thread;
use std::str;
use std::fs::File;
use std::io::{Read, Error, ErrorKind, Write};
use std::sync::Arc;

use packet::Packet;
use packet::data::TftpData;
use packet::data;
use packet::ack::TftpAck;
use packet::error::TftpError;
use codes::{ErrorCode, TransferMode, Opcode};

pub trait Callback<T: ?Sized>: Sync + Send {
    fn call(&self, arg: &Path);
}

impl<F, T: ?Sized> Callback<T> for F where F: Fn(&Path), F: Sync + Send {
    fn call(&self, arg: &Path) {
        self(arg)
    }
}

#[derive(Clone)]
struct Config {
    root: PathBuf,

    file_read_started_callback:    Option<Arc<Callback<Path>>>,
    file_write_started_callback:   Option<Arc<Callback<Path>>>,
    file_read_completed_callback:  Option<Arc<Callback<Path>>>,
    file_write_completed_callback: Option<Arc<Callback<Path>>>,
}

pub struct TftpServer {
    socket: UdpSocket,
    config: Config
}

type PacketBuff = [u8; 1024];
impl TftpServer {

    /// Create a TFTP server that serves from the given path. This server
    /// will not respond to requests until `start` is called.
    ///
    /// # Failures
    /// Returns `Err` if an error occurs while binding to the given address
    pub fn new<A: ToSocketAddrs, S: AsRef<OsStr> + ?Sized>(addr: A, root: &S)
                                                           -> Result<TftpServer, Error> {
        let socket = match UdpSocket::bind(addr) {
            Ok(s) => s,
            Err(e) => return Err(e)
        };
        Ok(TftpServer {
            socket: socket,
            config: Config {
                root: PathBuf::from(root),
                file_read_started_callback:    None,
                file_write_started_callback:   None,
                file_read_completed_callback:  None,
                file_write_completed_callback: None
            }
        })
    }

    /// Start the server. Requests will be handled in separate threads.
    pub fn start(&self) -> ! {
        loop {
            let mut packet_buffer = [0u8; 1024];
            let (count, addr) = self.socket.recv_from(&mut packet_buffer).unwrap();
            self.handle_request(addr, packet_buffer, count);
        }
    }

    /// Set a callback function to be invoked when a request is made to read
    /// a file. This callback will be passed the `Path` of the requested file.
    pub fn on_read_started<F: Callback<Path> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_read_started_callback = Some(Arc::new(callback));
        self
    }

    /// Set a callback function to be invoked when a request to read a file
    /// has been fulfilled. This callback will be passed the `Path` of the
    /// requested file.
    pub fn on_read_completed<F: Callback<Path> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_read_completed_callback = Some(Arc::new(callback));
        self
    }

    /// Set a callback function to be invoked when a request is made to read
    /// a file. This callback will be passed the `Path` of the requested file.
    pub fn on_write_started<F: Callback<Path> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_write_started_callback = Some(Arc::new(callback));
        self
    }

    /// Set a callback function to be invoked when a request to write a file
    /// has been fulfilled. This callback will be passed the `Path` of the
    /// requested file.
    pub fn on_write_completed<F: Callback<Path> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_write_completed_callback = Some(Arc::new(callback));
        self
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
        let config = self.config.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let (filename, mode) = match Self::parse_rw_request(&mut inner_packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    match socket.send_to(&e.as_packet(), addr) {
                        _ => return ()
                    }
                }
            };

            let full_path = config.root.join(filename);

            match config.file_write_started_callback {
                Some(callback) => callback.call(&full_path),
                None => ()
            }

            match Self::recieve_file(&socket, &full_path, &mode, addr) {
                Ok(_) => (),

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {socket.send_to(&err.as_packet(), addr).unwrap();}
            }

            match config.file_write_completed_callback {
                Some(callback) => callback.call(&full_path),
                None => ()
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
                    let _ = socket.send_to(&TftpError{
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
        let config = self.config.clone();
        thread::spawn(move || {
            let mut inner_packet = packet;
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let (filename, mode) = match Self::parse_rw_request(&mut inner_packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    match socket.send_to(&e.as_packet(), addr) {
                        _ => return ()
                    }
                }
            };

            let full_path = config.root.join(filename);

            match config.file_read_started_callback {
                Some(callback) => callback.call(&full_path),
                None => ()
            }

            match Self::send_file(&socket, &full_path, &mode, addr) {
                Ok(_) => (),

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {
                    match socket.send_to(&err.as_packet(), addr) {
                        _ => return ()
                    }
                }
            }

            match config.file_read_completed_callback {
                Some(callback) => callback.call(&full_path),
                None => ()
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
