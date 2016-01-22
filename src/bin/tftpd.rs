extern crate rustc_serialize;
extern crate docopt;

extern crate tftp;

use std::time::Duration;
use std::path::Path;
use std::fs::File;

use docopt::Docopt;
use tftp::server::TftpServer;

const USAGE: &'static str = "

Usage:
  tftpd <root> [<ip> [<port>]] [--retry=<retry>] [--read-timeout=<read_timeout>]
  tftpd (-h | --help)
  tftpd --version

Options:
  -h --help                         Show this screen
  --version                         Show version
  --retry=<retry>                   Number of times to retry sending/acknowledging a packet before giving up
  --read-timeout=<read_timeout>     Time (in ms) allowed before a packed is considered 'lost'
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_root: String,
    arg_ip: Option<String>,
    arg_port: Option<u32>,
    arg_retry: Option<u8>,
    arg_read_timeout: Option<u64>
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let ip = args.arg_ip.unwrap_or("0.0.0.0".to_string());
    let port = args.arg_port.unwrap_or(69).to_string();

    let addr = ip + ":" + &port;

    let mut server = TftpServer::new(&*addr, &args.arg_root).unwrap();

    if args.arg_retry.is_some() {
        server.set_send_retry_attempts(args.arg_retry.unwrap());
    }

    if args.arg_read_timeout.is_some() {
        server.set_read_timeout(
            Some(Duration::from_millis(args.arg_read_timeout.unwrap())));
    }

    server.on_write_started(|p: &Path, _: &File| {
        println!("Started write request for: {}", p.to_str().unwrap())
    }).on_write_completed(|p: &Path, _: &File| {
        println!("Completed write request for: {}", p.to_str().unwrap())
    });

    server.on_read_started(|p: &Path, _: &File| {
        println!("Started read request for: {}", p.to_str().unwrap())
    }).on_read_completed(|p: &Path, _: &File| {
        println!("Completed read request for: {}", p.to_str().unwrap())
    });

    server.start();
}
