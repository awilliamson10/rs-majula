use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(IfSetText, Buffered, VarShort)]
pub struct IfSetText<'a> {
    pub com: u16,
    pub text: &'a str,
}

impl ServerProtMessage for IfSetText<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.com);
        buf.pjstr(self.text, 10);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.com) + self.text.len() + 1
    }
}
