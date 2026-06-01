use crate::network::game::client::ClientProtMessage;
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::client_prot;

#[client_prot(Fixed(6), ClientEvent)]
pub struct EventCameraPosition {
    pub camera_pitch: u16,
    pub camera_yaw: u16,
    pub minimap_angle: u8,
    pub minimap_zoom: u8,
}

impl ClientProtMessage for EventCameraPosition {
    fn decode(buf: &mut Packet, _: usize) -> Self {
        EventCameraPosition {
            camera_pitch: buf.g2(),
            camera_yaw: buf.g2(),
            minimap_angle: buf.g1(),
            minimap_zoom: buf.g1(),
        }
    }
}
