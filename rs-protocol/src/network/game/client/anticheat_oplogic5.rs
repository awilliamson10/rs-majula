use crate::network::game::client::ClientProtMessage;
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::client_prot;

#[client_prot(Fixed, ClientEvent)]
pub struct AnticheatOpLogic5;

impl ClientProtMessage for AnticheatOpLogic5 {
    fn decode(_: &mut Packet, _: usize) -> Self {
        AnticheatOpLogic5
    }
}
