use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(IfSetTabActive, Buffered, Fixed)]
pub struct IfSetTabActive {
    pub tab: u8,
}

impl ServerProtMessage for IfSetTabActive {
    fn encode(&self, buf: &mut Packet) {
        buf.p1(self.tab);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.tab)
    }
}
