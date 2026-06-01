use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(MidiJingle, Buffered, VarShort)]
pub struct MidiJingle<'a> {
    pub delay: u16,
    pub bytes: &'a [u8],
}

impl ServerProtMessage for MidiJingle<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.delay);
        buf.pdata(self.bytes, 0, self.bytes.len());
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.delay) + self.bytes.len()
    }
}
