use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateZonePartialFollows, Immediate, Fixed)]
pub struct UpdateZonePartialFollows {
    pub x: u8,
    pub z: u8,
}

impl ServerProtMessage for UpdateZonePartialFollows {
    fn encode(&self, buf: &mut Packet) {
        buf.p1(self.x);
        buf.p1(self.z);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.x) + size_of_val(&self.z)
    }
}
