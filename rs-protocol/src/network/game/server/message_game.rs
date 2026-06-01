use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(MessageGame, Immediate, VarByte)]
pub struct MessageGame<'a> {
    pub text: &'a str,
}

impl ServerProtMessage for MessageGame<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.pjstr(self.text, 10);
    }

    fn sizeof(&self) -> usize {
        self.text.len() + 1
    }
}
