use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(IfSetPosition, Buffered, Fixed)]
pub struct IfSetPosition {
    pub com: u16,
    pub x: u16,
    pub y: u16,
}

impl ServerProtMessage for IfSetPosition {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.com);
        buf.p2(self.x);
        buf.p2(self.y);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.com) + size_of_val(&self.x) + size_of_val(&self.y)
    }
}
