use rs_io::Packet;
use rs_pack::types::{LocAngle, LocShape};
use rs_protocol::network::game::server::ServerProtMessage;
use rs_protocol::network::game::server::loc_add_change::LocAddChange;
use rs_protocol::network::game::server::loc_anim::LocAnim;
use rs_protocol::network::game::server::loc_del::LocDel;
use rs_protocol::network::game::server::loc_merge::LocMerge;
use rs_protocol::network::game::server::map_anim::MapAnim;
use rs_protocol::network::game::server::map_projanim::MapProjAnim;
use rs_protocol::network::game::server::obj_add::ObjAdd;
use rs_protocol::network::game::server::obj_count::ObjCount;
use rs_protocol::network::game::server::obj_del::ObjDel;
use rs_protocol::network::game::server::obj_reveal::ObjReveal;

/// A protocol message payload representing a zone update to be sent to clients.
///
/// Each variant wraps a specific server protocol message struct. These are
/// queued as part of [`ZoneEvent`](crate::zone_event::ZoneEvent)s and serialized
/// into network packets during the zone output phase of each engine tick.
#[derive(Debug, Clone)]
pub enum ZoneMessage {
    ObjAdd(ObjAdd),
    ObjDel(ObjDel),
    ObjCount(ObjCount),
    ObjReveal(ObjReveal),
    LocAddChange(LocAddChange),
    LocDel(LocDel),
    LocAnim(LocAnim),
    LocMerge(LocMerge),
    MapAnim(MapAnim),
    MapProjAnim(MapProjAnim),
}

impl ZoneMessage {
    /// Serializes this zone message into a packet buffer, prefixed with the protocol opcode.
    ///
    /// Writes a 1-byte protocol identifier followed by the message-specific payload.
    /// Used by [`Zone::compute_shared`](crate::zone::Zone::compute_shared) to build
    /// the shared byte buffer for enclosed events.
    ///
    /// # Arguments
    ///
    /// * `buf` -- The packet buffer to write into. Must have sufficient capacity
    ///   (see [`sizeof_zone`](Self::sizeof_zone)).
    ///
    /// # Side Effects
    ///
    /// Advances `buf.pos` by `1 + message.sizeof()` bytes.
    pub fn encode_zone(&self, buf: &mut Packet) {
        match self {
            Self::ObjAdd(m) => encode(m, buf),
            Self::ObjDel(m) => encode(m, buf),
            Self::ObjCount(m) => encode(m, buf),
            Self::ObjReveal(m) => encode(m, buf),
            Self::LocAddChange(m) => encode(m, buf),
            Self::LocDel(m) => encode(m, buf),
            Self::LocAnim(m) => encode(m, buf),
            Self::LocMerge(m) => encode(m, buf),
            Self::MapAnim(m) => encode(m, buf),
            Self::MapProjAnim(m) => encode(m, buf),
        }
    }

    /// Returns the total serialized size of this zone message in bytes.
    ///
    /// Includes the 1-byte protocol opcode prefix plus the message-specific payload size.
    /// Used to pre-allocate the shared byte buffer in
    /// [`Zone::compute_shared`](crate::zone::Zone::compute_shared).
    ///
    /// # Returns
    ///
    /// The total byte count: `1 + message.sizeof()`.
    pub fn sizeof_zone(&self) -> usize {
        1 + match self {
            Self::ObjAdd(m) => m.sizeof(),
            Self::ObjDel(m) => m.sizeof(),
            Self::ObjCount(m) => m.sizeof(),
            Self::ObjReveal(m) => m.sizeof(),
            Self::LocAddChange(m) => m.sizeof(),
            Self::LocDel(m) => m.sizeof(),
            Self::LocAnim(m) => m.sizeof(),
            Self::LocMerge(m) => m.sizeof(),
            Self::MapAnim(m) => m.sizeof(),
            Self::MapProjAnim(m) => m.sizeof(),
        }
    }
}

/// Writes a protocol opcode byte followed by the encoded message payload into `buf`.
///
/// # Arguments
///
/// * `message` -- The server protocol message to encode.
/// * `buf` -- The packet buffer to write into.
///
/// # Side Effects
///
/// Advances `buf.pos` by `1 + message.sizeof()` bytes.
fn encode<M: ServerProtMessage>(message: &M, buf: &mut Packet) {
    buf.p1(M::PROT as u8);
    message.encode(buf);
}

/// Packs tile-local x and z coordinates into a single byte for zone protocol messages.
///
/// The zone-local coordinate uses the lower 3 bits of each axis (range 0..7),
/// packing x into the upper nibble and z into the lower nibble.
///
/// # Arguments
///
/// * `x` -- The absolute x coordinate; only the lower 3 bits (`x & 7`) are used.
/// * `z` -- The absolute z coordinate; only the lower 3 bits (`z & 7`) are used.
///
/// # Returns
///
/// A packed byte: `(x & 7) << 4 | (z & 7)`.
///
/// **Called by:** `Zone::add_obj`, `Zone::reveal_obj`, `Zone::remove_obj_at`,
/// `Zone::respawn_obj`, `Engine` methods for obj/loc/map operations,
/// `ActivePlayer::update_zones`.
#[inline]
pub fn pack_zone_coord(x: u16, z: u16) -> u8 {
    ((x & 7) << 4) as u8 | (z & 7) as u8
}

/// Packs a loc shape and angle into a single byte for zone protocol messages.
///
/// The shape occupies the upper 6 bits and the angle the lower 2 bits.
///
/// # Arguments
///
/// * `shape` -- The loc's shape variant (e.g., wall, centrepiece).
/// * `angle` -- The loc's rotation angle (North, East, South, West).
///
/// # Returns
///
/// A packed byte: `(shape << 2) | (angle & 3)`.
///
/// **Called by:** `Engine` methods for loc merge operations.
#[inline]
pub fn pack_shape_angle(shape: LocShape, angle: LocAngle) -> u8 {
    ((shape as u8) << 2) | (angle as u8 & 3)
}
