use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(EnableTracking, Buffered, Fixed)]
pub struct EnableTracking;

impl ServerProtMessage for EnableTracking {
    fn encode(&self, _: &mut Packet) {}

    fn sizeof(&self) -> usize {
        0
    }
}
