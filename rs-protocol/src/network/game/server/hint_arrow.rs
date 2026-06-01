use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(HintArrow, Buffered, Fixed)] // TODO: what should priority be?
pub struct HintArrow {
    pub hint: u8,
    pub arg1: u16,
    pub arg2: u16,
    pub arg3: u8,
}

impl ServerProtMessage for HintArrow {
    fn encode(&self, buf: &mut Packet) {
        buf.p1(self.hint);
        buf.p2(self.arg1);
        buf.p2(self.arg2);
        buf.p1(self.arg3);
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.hint)
            + size_of_val(&self.arg1)
            + size_of_val(&self.arg2)
            + size_of_val(&self.arg3)
    }
}
