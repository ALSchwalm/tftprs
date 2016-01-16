pub mod data;
pub mod ack;
pub mod error;

pub trait Packet: Sized {
    fn as_packet(&self) -> Vec<u8>;
    fn from_buffer(&[u8]) -> Option<Self>;
}
