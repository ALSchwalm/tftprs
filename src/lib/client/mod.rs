use std::net::UdpSocket;

struct TftpClient {
    socket: UdpSocket,
}

impl TftpClient {
    fn new() -> TftpClient {
        let socket = match UdpSocket::bind("0.0.0.0:0") {
            Ok(v) => v,
            Err(e) => {
                match e.kind() {
                    _ => panic!("error"),
                }
            }
        };
        TftpClient { socket: socket }
    }
}
