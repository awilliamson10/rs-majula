use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateRunEnergy, Buffered, Fixed)]
pub struct UpdateRunEnergy {
    pub energy: u8,
}

impl ServerProtMessage for UpdateRunEnergy {
    fn encode(&self, buf: &mut Packet) {
        buf.p1(self.energy);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.energy)
    }
}
