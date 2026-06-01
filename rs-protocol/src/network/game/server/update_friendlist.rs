use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateFriendList, Buffered, Fixed)]
pub struct UpdateFriendList {
    pub user37: i64,
    pub node: u8,
}

impl ServerProtMessage for UpdateFriendList {
    fn encode(&self, buf: &mut Packet) {
        buf.p8(self.user37);
        buf.p1(self.node);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.user37) + size_of_val(&self.node)
    }
}
