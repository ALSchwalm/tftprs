use std::net::{UdpSocket, ToSocketAddrs};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::thread;

enum TransferMode {
    NetAscii, Octet
}

struct TftpServer {
    socket: UdpSocket,
    root: PathBuf
}

impl TftpServer {
    fn new<A: ToSocketAddrs,
           S: AsRef<OsStr> + ?Sized>(addr: A,
                                     root: &S) -> TftpServer {
        let socket = match UdpSocket::bind(addr) {
            Ok(v) => v,
            Err(e) => match e.kind() {
                _ => panic!("error")
            }
        };
        TftpServer{socket:socket, root:PathBuf::from(root)}
    }

    fn start(self) {
        thread::spawn(move || {
            self.socket.recv_from(&mut [10])
        });
    }

    fn handle_read(self) {

    }
}

fn main() {
    let server = TftpServer::new("127.0.0.1:34254", "/home/adam/Public");

    // let mut buf = [0; 10];
    // let (amt, src) = socket.recv_from(&mut buf).unwrap();

    // // Send a reply to the socket we received data from
    // let buf = &mut buf[..amt];
    // buf.reverse();
    // socket.send_to(buf, &src).unwrap();
}
