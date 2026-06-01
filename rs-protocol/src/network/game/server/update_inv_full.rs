use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;

#[server_prot(UpdateInvFull, Immediate, VarShort)]
pub struct UpdateInvFull<'a> {
    pub com: u16,
    pub objs: &'a [Option<(u16, i32)>],
}

impl ServerProtMessage for UpdateInvFull<'_> {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.com);
        buf.p1(self.objs.len() as u8);
        for obj in self.objs {
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
            + 1
            + self
                .objs
                .iter()
                .map(|obj| match obj {
                    None => 3,
                    Some(obj) => 2 + if obj.1 >= u8::MAX as i32 { 5 } else { 1 },
                })
                .sum::<usize>()
    }
}
