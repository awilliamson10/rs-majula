use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(DataLoc, Immediate, VarShort)]
pub struct DataLoc<'a> {
    pub x: u8,
    pub z: u8,
    pub off: u16,
    pub len: u16,
    pub data: &'a [u8],
}

impl ServerProtMessage for DataLoc<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.p1(self.x);
        buf.p1(self.z);
        buf.p2(self.off);
        buf.p2(self.len);
        buf.pdata(self.data, 0, self.data.len());
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.x)
            + size_of_val(&self.z)
            + size_of_val(&self.off)
            + size_of_val(&self.len)
            + self.data.len()
    }
}
