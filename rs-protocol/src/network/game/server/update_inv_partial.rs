use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateInvPartial, Immediate, VarShort)]
pub struct UpdateInvPartial<'a> {
    pub com: u16,
    /// Each entry is `(slot, item)` where `slot` is the actual inventory slot index
    /// being updated (not a sequential position) and `item` is its new contents.
    pub objs: &'a [(u16, Option<(u16, i32)>)],
}

impl ServerProtMessage for UpdateInvPartial<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.com);
        for (slot, obj) in self.objs {
            buf.p1(*slot as u8);

            match obj {
                None => {
                    buf.p2(0);
                    buf.p1(0);
                }
                Some(obj) => {
                    buf.p2(obj.0.saturating_add(1));
                    let count = obj.1;
                    if count >= u8::MAX as i32 {
                        buf.p1(u8::MAX);
                        buf.p4(count);
                    } else {
                        buf.p1(count as u8);
                    }
                }
            }
        }
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.com)
            + self.objs.len()
            + self
                .objs
                .iter()
                .map(|(_, obj)| match obj {
                    None => 3,
                    Some(obj) => 2 + if obj.1 >= u8::MAX as i32 { 5 } else { 1 },
                })
                .sum::<usize>()
    }
}
