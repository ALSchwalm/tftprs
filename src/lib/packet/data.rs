use packet::Packet;

#[derive(Debug, PartialEq, Eq)]
pub struct TftpData {
    pub number: u16,
    pub data: Vec<u8>,
}

pub const MAX_DATA_SIZE: usize = 512;

impl Packet for TftpData {
    fn as_packet(&self) -> Vec<u8> {
        let high = (self.number >> 8) as u8;
        let low = self.number as u8;

        let mut packet = vec![0u8, 3u8, high, low];
        packet.extend(self.data.iter());
        packet
    }

    fn from_buffer(buf: &[u8]) -> Option<TftpData> {
        if buf.len() < 4 {
            return None
        } else if buf[0] != 0u8 || buf[1] != 3u8 {
            return None
        }
        Some(TftpData {
            number: ((buf[2] as u16) << 8) | buf[3] as u16,
            data: buf[4..].to_vec()
        })
    }
}

#[test]
fn tftp_data_round_trip() {
    let data = TftpData{
        number: 1u16,
        data: vec![0u8; MAX_DATA_SIZE]
    };
    let roundtrip = TftpData::from_buffer(&data.as_packet()).unwrap();

    assert_eq!(data, roundtrip);
}
