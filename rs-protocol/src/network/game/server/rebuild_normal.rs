use crate::network::game::server::ServerProtMessage;
use crate::network::game::server_prot::ServerProt;
use crate::network::game::server_prot_message::ServerProtMessageInfo;
use crate::network::game::server_prot_priority::ServerProtPriority;
use rs_io::{Packet, PacketFrame};
use rs_protocol_macros::server_prot;
use std::collections::{HashMap, HashSet};

#[server_prot(RebuildNormal, Immediate, VarShort)]
pub struct RebuildNormal {
    pub zone_x: u16,
    pub zone_z: u16,
    pub mapsquares: HashSet<u16>,
    pub crcs: HashMap<(char, u8, u8), i32>,
}

impl ServerProtMessage for RebuildNormal {
    fn encode(&self, buf: &mut Packet) {
        buf.p2(self.zone_x);
        buf.p2(self.zone_z);
        for mapsquare in &self.mapsquares {
            let x = (mapsquare >> 8) as u8;
            let z = (mapsquare & 0xFF) as u8;
            buf.p1(x);
            buf.p1(z);
            match self.crcs.get(&('m', x, z)).copied() {
                None => buf.p4(0),
                Some(crc) => buf.p4(crc),
            }
            match self.crcs.get(&('l', x, z)).copied() {
                None => buf.p4(0),
                Some(crc) => buf.p4(crc),
            }
        }
    }

    fn sizeof(&self) -> usize {
        size_of_val(&self.zone_x) + size_of_val(&self.zone_z) + (self.mapsquares.len() * 10)
    }
}
