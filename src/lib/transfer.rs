use std::net::{UdpSocket, SocketAddr};
use std::fs::File;
use std::io::{Write, Read};
use std::path::PathBuf;

use config::Config;
use packet::error::{TftpError, translate_io_error};
use codes::{ErrorCode, TransferMode};
use packet::Packet;
use packet::data;
use packet::data::TftpData;
use packet::ack::TftpAck;

// Receive a file at `path` from `addr`. If the file is successfully received,
// Ok(file) is returned. Otherwise, a TftpError is returned.
pub fn recieve_file(config: &Config, socket: &UdpSocket, path: &PathBuf,
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

    if let Some(ref callback) = config.file_write_started_callback {
        callback.call(&path, &file);
    }

    // The buffer to receive data into. Max size is 512 payload bytes plus
    // 2 for opcode and 2 for
    let mut resp_buffer = [0u8; data::MAX_DATA_SIZE + 2 + 2];
    for number in 0.. {
        let mut attempts = 0;
        while attempts <= config.send_retry_attempts {
            attempts += 1;

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
                continue;
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
                if let Err(_) = file.write_all(&data.data) {
                    return Err(TftpError{
                        code: ErrorCode::Undefined,
                        message: None
                    });
                }

                if data.data.len() < data::MAX_DATA_SIZE {

                    // No further packets, so stop
                    let ack = TftpAck{number: number+1};
                    socket.send_to(&ack.as_packet(), addr).unwrap();
                    return Ok(file);
                }
                break;
            }
        }
        if attempts > config.send_retry_attempts {
            return Err(TftpError{
                code: ErrorCode::Undefined,
                message: Some("Exceeded max send attempts".to_string())
            });
        }
    }
    unreachable!();
}

// Send the file at `path` to `target_addr`. If the transfer completes
// successfully, Ok(file) is returned. Otherwise, a TftpError is returned.
pub fn send_file(config: &Config, socket: &UdpSocket, path: &PathBuf,
                 _: &TransferMode, target_addr: SocketAddr) -> Result<File, TftpError> {
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

    if let Some(ref callback) = config.file_read_started_callback {
        callback.call(&path, &file);
    }

    // We just need a 4 byte buffer for the ACK
    //FIXME: this allows larger messages that happen to start
    //       with the right bytes to be accepted
    let mut resp_buffer = [0u8; 4];
    let mut previous_bytes_sent = 0;

    let mut data_packet = TftpData{
        number: 0,
        data: vec![0u8; data::MAX_DATA_SIZE]
    };

    for number in 1.. {
        data_packet.number = number;
        data_packet.data.reserve(data::MAX_DATA_SIZE);

        let file_bytes = match file.read(&mut data_packet.data) {
            Ok(b) => b,
            Err(e) => return Err(translate_io_error(e.kind()))
        };

        // If this is the end of the file (we've read less than 512 bytes),
        // then truncate the data vector so the packet won't be padded with
        // zeros
        //FIXME: read may return less than MAX_DATA_SIZE even when the file
        // is not empty. Should probably read in a loop (or find a way to
        // do this with read_exact).
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
            match send_data_packet(&config, &data_packet, socket,
                                   &target_addr, &mut resp_buffer) {
                Ok(()) => (),
                Err(e) => return Err(e)
            }
        }
    }
    unreachable!();
}

// Send the TftpData packet `packet` to `target_addr` until an ACK is
// received or 'send_retry_attempts' is exceeded.
fn send_data_packet(config: &Config, packet: &TftpData, socket: &UdpSocket,
                    target_addr: &SocketAddr, resp_buffer: &mut [u8]) -> Result<(), TftpError> {

    let expected_ack = TftpAck{number:packet.number};
    // Loop until we receive an ACK from the appropriate source
    let mut attempts = 0;
    while attempts <= config.send_retry_attempts {
        attempts += 1;

        socket.send_to(&packet.as_packet(), target_addr).unwrap();
        let (count, resp_addr) = socket.recv_from(resp_buffer).unwrap();

        let actual_ack = match TftpAck::from_buffer(&resp_buffer[..count]) {
            Some(a) => a,
            None => continue
        };

        // Receiving a packet from unexpected source does not
        // interrupt the operation with the current client
        if &resp_addr != target_addr {
            let _ = socket.send_to(&TftpError{
                code: ErrorCode::UnknownTransferID,
                message: None
            }.as_packet(), resp_addr);
        } else if expected_ack == actual_ack {
            // The fragment has been sent and acknowledged
            break;
        }
    }
    if attempts > config.send_retry_attempts {
        return Err(TftpError{
            code: ErrorCode::Undefined,
            message: Some("Exceeded max send attempts".to_string())
        });
    }
    Ok(())
}
