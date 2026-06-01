use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateRunWeight, Buffered, Fixed)]
pub struct UpdateRunWeight {
    pub kg: u16,
}

impl ServerProtMessage for UpdateRunWeight {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.kg);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.kg)
    }
}
