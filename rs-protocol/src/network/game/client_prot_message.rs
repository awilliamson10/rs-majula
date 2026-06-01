use crate::network::game::client_prot_category::ClientProtCategory;
use rs_io::{Packet, PacketFrame};

pub trait ClientProtMessageInfo {
    const FRAME: (PacketFrame, Option<u8>);
    const CATEGORY: ClientProtCategory;
}

pub trait ClientProtMessage: ClientProtMessageInfo {
    fn decode(buf: &mut Packet, len: usize) -> Self;
}
