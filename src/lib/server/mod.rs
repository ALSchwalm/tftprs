use std::net::{UdpSocket, SocketAddr, ToSocketAddrs};
use std::ffi::OsStr;
use std::path::{PathBuf, Path};
use std::thread;
use std::str;
use std::fs::File;
use std::io::{Read, Error, Write};
use std::sync::Arc;
use std::time::Duration;

use packet::{Packet, PacketBuff, get_packet_opcode};
use packet::data::TftpData;
use packet::data;
use packet::ack::TftpAck;
use packet::error::{TftpError, translate_io_error};
use codes::{ErrorCode, TransferMode, Opcode};

/// A simple trait representing a callable that will be invoked after some
/// event has occurred.
pub trait Callback<T: ?Sized, U: ?Sized>: Sync + Send {
    fn call(&self, arg1: &T, arg2: &U);
}

/// A default implementation for Fn
impl<F, T: ?Sized, U: ?Sized> Callback<T, U> for F where F: Fn(&T, &U), F: Sync + Send {
    fn call(&self, arg1: &T, arg2: &U) {
        self(arg1, arg2)
    }
}

#[derive(Clone)]
struct Config {
    root: PathBuf,

    file_read_started_callback:    Option<Arc<Callback<Path, File>>>,
    file_write_started_callback:   Option<Arc<Callback<Path, File>>>,
    file_read_completed_callback:  Option<Arc<Callback<Path, File>>>,
    file_write_completed_callback: Option<Arc<Callback<Path, File>>>,

    read_timeout: Option<Duration>,
    send_retry_attempts: u8
}

pub struct TftpServer {
    socket: UdpSocket,
    config: Config
}


impl TftpServer {

    /// Create a TFTP server that serves from the given path. This server
    /// will not respond to requests until `start` is called.
    ///
    /// By default, the worker thread will timeout after 20ms, and attempt
    /// to re-send an unacknowledged packet up to 5 times.
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
                file_write_completed_callback: None,

                read_timeout: Some(Duration::from_millis(20)),
                send_retry_attempts: 5
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
    /// a file. This callback will be passed the `File` being read, and its
    /// `Path`.
    pub fn on_read_started<F: Callback<Path, File> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_read_started_callback = Some(Arc::new(callback));
        self
    }

    /// Set a callback function to be invoked when a request to read a file
    /// has been fulfilled. This callback will be passed the `File` that was
    /// read and its `Path`.
    pub fn on_read_completed<F: Callback<Path, File> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_read_completed_callback = Some(Arc::new(callback));
        self
    }

    /// Set a callback function to be invoked when a request is made to read
    /// a file. This callback will be passed the `File` being written and
    /// its `Path`.
    pub fn on_write_started<F: Callback<Path, File> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_write_started_callback = Some(Arc::new(callback));
        self
    }

    /// Set a callback function to be invoked when a request to write a file
    /// has been fulfilled. This callback will be passed the `File` that was
    /// written and its `Path`.
    pub fn on_write_completed<F: Callback<Path, File> + 'static>(&mut self, callback: F) -> &mut Self {
        self.config.file_write_completed_callback = Some(Arc::new(callback));
        self
    }

    /// Sets the read timeout to the timeout specified.
    /// If the value specified is None, then read calls will block indefinitely.
    ///
    /// It is an error to pass the zero Duration to this method.
    pub fn set_read_timeout(&mut self, dur: Option<Duration>) {
        self.config.read_timeout = dur;
    }

    /// Set the number of times the server will attempt to re-transmit a packet
    /// that did not receive and ACK.
    pub fn set_send_retry_attempts(&mut self, attempts: u8) {
        self.config.send_retry_attempts = attempts;
    }

    // Dispatch an incoming request to the appropriate handler. Does nothing
    // if the packet is ill-formed or unexpected.
    fn handle_request(&self, addr: SocketAddr, packet: PacketBuff, length: usize) {
        let code = get_packet_opcode(length, &packet);
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

    // Extract the transfer mode and path from the given packet
    fn parse_rw_request(packet: &PacketBuff, length: usize) -> Result<(&str, TransferMode), TftpError> {
        let packet = &packet[2..length];
        let mut parts = packet.splitn(3, |x| *x == 0);

        let filename = match parts.next() {
            Some(filename_buff) => str::from_utf8(filename_buff).unwrap(),
            None => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Invalid filename")
            })
        };

        let mode_str = match parts.next(){
            Some(mode_str_buff) => str::from_utf8(mode_str_buff).unwrap(),
            None => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Invalid mode string")
            })
        };

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
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            socket.set_read_timeout(config.read_timeout).unwrap();

            let (filename, mode) = match Self::parse_rw_request(&packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    match socket.send_to(&e.as_packet(), addr) {
                        _ => return ()
                    }
                }
            };

            let full_path = config.root.join(filename);

            let file = match Self::recieve_file(&config, &socket, &full_path, &mode, addr) {
                Ok(f) => f,

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {
                    socket.send_to(&err.as_packet(), addr).unwrap();
                    return ();
                }
            };

            match config.file_write_completed_callback {
                Some(callback) => callback.call(&full_path, &file),
                None => ()
            }
        });
    }

    fn recieve_file(config: &Config, socket: &UdpSocket, path: &PathBuf,
                    _: &TransferMode, addr: SocketAddr) -> Result<File, TftpError> {
        if path.exists() {
            return Err(TftpError{
                code: ErrorCode::FileExists,
                message: None
            });
        }

        let mut file = match File::create(path) {
            Ok(f) => f,
            Err(e) => return Err(translate_io_error(e.kind()))
        };

        match config.file_write_started_callback {
            Some(ref callback) => callback.call(&path, &file),
            None => ()
        }

        // The buffer to receive data into. Max size is 512 payload bytes plus
        // 2 for opcode and 2 for
        let mut resp_buffer = [0u8; data::MAX_DATA_SIZE + 2 + 2];
        for number in 0.. {
            for _ in 0..config.send_retry_attempts {
                let ack = TftpAck{number: number};
                socket.send_to(&ack.as_packet(), addr).unwrap();

                let (count, resp_addr) = match socket.recv_from(&mut resp_buffer) {
                    Ok(r) => r,

                    // Different platforms are allowed to return different
                    // error codes for timeouts, so just assume any error
                    // is a timeout and try again
                    Err(_) => continue
                };

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
                if data.number != number+1 {
                    continue;
                } else {

                    // This is the expected packet, so write it out
                    file.write(&data.data);
                    if data.data.len() < data::MAX_DATA_SIZE {
                        let ack = TftpAck{number: number+1};
                        socket.send_to(&ack.as_packet(), addr).unwrap();

                        // No further packets, so stop
                        return Ok(file);
                    }
                    break;
                }
            }
        }
        unreachable!();
    }

    fn handle_read_request(&self, addr: SocketAddr, packet: PacketBuff, length: usize) {
        let config = self.config.clone();
        thread::spawn(move || {
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            socket.set_read_timeout(config.read_timeout).unwrap();

            let (filename, mode) = match Self::parse_rw_request(&packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    match socket.send_to(&e.as_packet(), addr) {
                        _ => return ()
                    }
                }
            };

            let full_path = config.root.join(filename);

            let file = match Self::send_file(&config, &socket, &full_path, &mode, addr) {
                Ok(f) => f,

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {
                    match socket.send_to(&err.as_packet(), addr) {
                        _ => return ()
                    }
                }
            };

            match config.file_read_completed_callback {
                Some(callback) => callback.call(&full_path, &file),
                None => ()
            }
        });
    }

    fn send_file(config: &Config, socket: &UdpSocket, path: &PathBuf,
                 _: &TransferMode, addr: SocketAddr) -> Result<File, TftpError> {
        if !path.exists() {
            return Err(TftpError{
                code: ErrorCode::FileNotFound,
                message: None
            });
        }

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => return Err(translate_io_error(e.kind()))
        };

        match config.file_read_started_callback {
            Some(ref callback) => callback.call(&path, &file),
            None => ()
        }

        // We just need a 4 byte buffer for the ACK
        //FIXME: this allows larger messages that happen to start
        //       with the right bytes to be accepted
        let mut resp_buffer = [0u8; 4];
        let mut previous_bytes_sent = 0;

        for number in 1.. {
            let mut data_packet = TftpData{
                number: number,
                data: vec![0u8; data::MAX_DATA_SIZE]
            };

            let file_bytes = match file.read(&mut data_packet.data) {
                Ok(b) => b,
                Err(e) => return Err(translate_io_error(e.kind()))
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
            if file_bytes == 0 && number > 1 && previous_bytes_sent < data::MAX_DATA_SIZE {
                return Ok(file);
            } else {
                previous_bytes_sent = file_bytes;
                let expected_ack = TftpAck{number:number};

                // Loop until we receive an ACK from the appropriate source
                for _ in 0..config.send_retry_attempts {
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
        unreachable!();
    }
}
