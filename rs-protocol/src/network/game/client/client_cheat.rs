use crate::network::game::client::ClientProtMessage;
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::client_prot;

#[client_prot(VarByte, UserEvent)]
pub struct ClientCheat {
    pub cheat: String,
}

impl ClientProtMessage for ClientCheat {
    fn decode(buf: &mut Packet, _: usize) -> Self {
        ClientCheat {
            cheat: buf.gjstr(10),
        }
    }
}
