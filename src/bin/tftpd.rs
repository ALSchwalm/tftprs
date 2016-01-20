extern crate tftp;
use tftp::server::TftpServer;

use std::path::Path;
use std::fs::File;

fn main() {
    let mut server = TftpServer::new("0.0.0.0:69", "/home/adam/Public").unwrap();
    server.on_read_started(|p: &Path, _: &File| {
        println!("Started read request for: {}", p.to_str().unwrap())
    }).on_read_completed(|p: &Path, _: &File| {
        println!("Completed read request for: {}", p.to_str().unwrap())
    });

    server.on_write_started(|p: &Path, _: &File| {
        println!("Started write request for: {}", p.to_str().unwrap())
    }).on_write_completed(|p: &Path, _: &File| {
        println!("Completed write request for: {}", p.to_str().unwrap())
    });
    server.start();
}
