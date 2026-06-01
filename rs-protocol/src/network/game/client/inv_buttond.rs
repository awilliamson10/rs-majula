use crate::network::game::client::ClientProtMessage;
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::client_prot;

#[client_prot(Fixed(6), UserEvent)]
pub struct InvButtonD {
    pub com: u16,
    pub slot: u16,
    pub slot2: u16,
}

impl ClientProtMessage for InvButtonD {
    fn decode(buf: &mut Packet, _: usize) -> Self {
        InvButtonD {
            com: buf.g2(),
            slot: buf.g2(),
            slot2: buf.g2(),
        }
    }
}
