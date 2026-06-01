use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(MessagePrivate, Immediate, VarByte)]
pub struct MessagePrivate<'a> {
    pub user37: i64,
    pub id: i32,
    pub level: u8,
    pub bytes: &'a [u8],
}

impl ServerProtMessage for MessagePrivate<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.p8(self.user37);
        buf.p4(self.id);
        buf.p1(self.level);
        buf.pdata(self.bytes, 0, self.bytes.len());
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.user37)
            + size_of_val(&self.id)
            + size_of_val(&self.level)
            + self.bytes.len()
    }
}
