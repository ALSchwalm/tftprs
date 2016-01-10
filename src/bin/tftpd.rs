extern crate tftp;
use tftp::server::TftpServer;

fn main() {
    let server = TftpServer::new("/home/adam/Public");
    server.start();
}
