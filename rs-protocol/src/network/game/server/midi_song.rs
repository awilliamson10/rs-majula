use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(MidiSong, Buffered, VarByte)]
pub struct MidiSong<'a> {
    pub name: &'a str,
    pub crc: i32,
    pub len: i32,
}

impl ServerProtMessage for MidiSong<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.pjstr(self.name, 10);
        buf.p4(self.crc);
        buf.p4(self.len);
    }

    fn sizeof(&self) -> usize {
        self.name.len() + 1 + size_of_val(&self.crc) + size_of_val(&self.len)
    }
}
