use std::net::{UdpSocket, SocketAddr, ToSocketAddrs};
use std::ffi::OsStr;
use std::path::{PathBuf, Path};
use std::thread;
use std::str;
use std::fs::File;
use std::io::Error;
use std::sync::Arc;
use std::time::Duration;

use codes::{ErrorCode, TransferMode, Opcode};
use packet::{Packet, PacketBuff, get_packet_opcode};
use packet::error::TftpError;
use transfer::{recieve_file, send_file};
use config::Config;
use callback::Callback;

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
        let socket = try!(UdpSocket::bind(addr));
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
        if let Ok(code) = get_packet_opcode(length, &packet) {
            match code {
                Opcode::ReadRequest => self.handle_read_request(addr, packet, length),
                Opcode::WriteRequest => self.handle_write_request(addr, packet, length),
                _ => (),
            }
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
                message: Some("Invalid filename".to_string())
            })
        };

        let mode_str = match parts.next(){
            Some(mode_str_buff) => str::from_utf8(mode_str_buff).unwrap(),
            None => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Invalid mode string".to_string())
            })
        };

        let mode = match &mode_str.to_lowercase()[..] {
            "netascii" => TransferMode::NetAscii,
            "octet" => TransferMode::Octet,
            "email" => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Email transfer mode is not supported".to_string())
            }),
            _ => return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Unknown transfer mode".to_string())
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
                    let _ = socket.send_to(&e.as_packet(), addr);
                    return ();
                }
            };

            let full_path = config.root.join(filename);

            let file = match recieve_file(&config, &socket, &full_path, &mode, addr) {
                Ok(f) => f,

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {
                    let _ = socket.send_to(&err.as_packet(), addr);
                    return ();
                }
            };

            if let Some(callback) = config.file_write_completed_callback {
                callback.call(&full_path, &file);
            }
        });
    }


    fn handle_read_request(&self, addr: SocketAddr, packet: PacketBuff, length: usize) {
        let config = self.config.clone();
        thread::spawn(move || {
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            socket.set_read_timeout(config.read_timeout).unwrap();

            let (filename, mode) = match Self::parse_rw_request(&packet, length) {
                Ok((f, m)) => (f, m),
                Err(e) => {
                    let _ = socket.send_to(&e.as_packet(), addr);
                    return ();
                }
            };

            let full_path = config.root.join(filename);

            let file = match send_file(&config, &socket, &full_path, &mode, addr) {
                Ok(f) => f,

                // Sending the error is a courtesy, so if it fails, don't
                // worry about it
                Err(err) => {
                    let _ = socket.send_to(&err.as_packet(), addr);
                    return ();
                }
            };

            if let Some(callback) = config.file_read_completed_callback {
                callback.call(&full_path, &file);
            }
        });
    }
}
