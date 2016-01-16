use packet::Packet;

#[derive(Debug, PartialEq, Eq)]
pub struct TftpAck {
    pub number: u16
}

impl Packet for TftpAck {
    fn as_packet(&self) -> Vec<u8> {
        vec![
            0x00,
            0x04,
            (self.number >> 8) as u8,
            self.number as u8
        ]
    }

    fn from_buffer(buf: &[u8]) -> Option<TftpAck> {
        if buf.len() != 4 {
            return None
        } else if buf[0] != 0u8 || buf[1] != 4u8 {
            return None
        }
        Some(TftpAck{
            number: ((buf[2] as u16 ) << 8) | buf[3] as u16
        })
    }
}

#[test]
fn tftp_ack_round_trip() {
    let ack = TftpAck{number: 10u16};
    let roundtrip = TftpAck::from_buffer(&ack.as_packet()).unwrap();

    assert_eq!(ack, roundtrip);
}
