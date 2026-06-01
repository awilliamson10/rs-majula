use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(PlayerInfo, Immediate, VarShort)]
pub struct PlayerInfo<'a> {
    pub bytes: &'a [u8],
}

impl ServerProtMessage for PlayerInfo<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.pdata(self.bytes, 0, self.bytes.len());
    }

    fn sizeof(&self) -> usize {
        self.bytes.len()
    }
}
