use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(SetMultiway, Buffered, Fixed)]
pub struct SetMultiway {
    pub hide: bool,
}

impl ServerProtMessage for SetMultiway {
    fn encode(&self, buf: &mut Packet) {
        buf.p1(self.hide as u8);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.hide)
    }
}
