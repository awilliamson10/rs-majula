use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(Logout, Immediate, Fixed)]
pub struct Logout;

impl ServerProtMessage for Logout {
    fn encode(&self, _: &mut Packet) {}

    fn sizeof(&self) -> usize {
        0
    }
}
