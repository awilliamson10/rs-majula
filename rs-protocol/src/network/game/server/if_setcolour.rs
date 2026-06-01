use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(IfSetColour, Buffered, Fixed)]
pub struct IfSetColour {
    pub com: u16,
    pub colour: u16,
}

impl ServerProtMessage for IfSetColour {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.com);
        buf.p2(self.colour);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.com) + size_of_val(&self.colour)
    }
}
