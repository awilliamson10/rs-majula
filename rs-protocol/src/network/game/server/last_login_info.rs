use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(LastLoginInfo, Buffered, Fixed)]
pub struct LastLoginInfo {
    pub ip: i32,
    pub login: u16,
    pub recovery: u8,
    pub messages: u16,
}

impl ServerProtMessage for LastLoginInfo {
    fn encode(&self, buf: &mut Packet) {
        buf.p4(self.ip);
        buf.p2(self.login);
        buf.p1(self.recovery);
        buf.p2(self.messages);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.ip)
            + size_of_val(&self.login)
            + size_of_val(&self.recovery)
            + size_of_val(&self.messages)
    }
}
