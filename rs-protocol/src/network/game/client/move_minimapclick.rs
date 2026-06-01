use crate::network::game::client::{ClientProtMessage, pack_coord};
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::client_prot;

#[client_prot(VarByte, UserEvent)]
pub struct MoveMinimapClick {
    pub path: Vec<u32>,
    pub ctrl: bool,
}

impl ClientProtMessage for MoveMinimapClick {
    fn decode(buf: &mut Packet, len: usize) -> Self {
        let ctrl = buf.g1();
        let x = buf.g2();
        let z = buf.g2();

        let waypoints = (len - buf.pos - 14) / 2;
        let mut path = Vec::with_capacity(1 + waypoints.min(24));
        path.push(pack_coord(x, z));

        for _ in 1..=waypoints.min(24) {
            path.push(pack_coord(
                x.wrapping_add_signed(buf.g1s() as i16),
                z.wrapping_add_signed(buf.g1s() as i16),
            ));
        }
        MoveMinimapClick {
            path,
            ctrl: ctrl != 0,
        }
    }
}
