use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateIgnoreList, Buffered, VarShort)]
pub struct UpdateIgnoreList<'a> {
    pub users37: &'a [i64],
}

impl ServerProtMessage for UpdateIgnoreList<'_> {
    fn encode(&self, buf: &mut Packet) {
        for name in self.users37 {
            buf.p8(*name);
        }
    }

    fn sizeof(&self) -> usize {
        8 * self.users37.len()
    }
}
