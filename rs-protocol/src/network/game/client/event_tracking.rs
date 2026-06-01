use crate::network::game::client::ClientProtMessage;
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::client_prot;

#[client_prot(VarShort, ClientEvent)]
pub struct EventTracking {
    pub bytes: Vec<u8>,
}

impl ClientProtMessage for EventTracking {
    fn decode(buf: &mut Packet, len: usize) -> Self {
        let mut bytes = Vec::with_capacity(len);
        buf.gdata(&mut bytes, 0, len);
        EventTracking { bytes }
    }
}
