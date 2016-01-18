extern crate tftp;
use tftp::server::TftpServer;

use std::path::Path;

fn main() {
    let mut server = TftpServer::new("0.0.0.0:69", "/home/adam/Public").unwrap();
    server.on_read_started(|p: &Path| {
        println!("Started read request for: {}", p.to_str().unwrap())
    }).on_read_completed(|p: &Path| {
        println!("Completed read request for: {}", p.to_str().unwrap())
    });

    server.on_write_started(|p: &Path| {
        println!("Started write request for: {}", p.to_str().unwrap())
    }).on_write_completed(|p: &Path| {
        println!("Completed write request for: {}", p.to_str().unwrap())
    });
    server.start();
}
