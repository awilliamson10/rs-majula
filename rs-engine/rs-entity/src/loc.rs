use crate::lifetime::EntityLifeTime;
use rs_grid::{CoordGrid, ZoneCoordGrid};
use rs_pack::types::{LocAngle, LocLayer, LocShape};

/// ---- THE BELOW SECTION IS FOR BUILDING THE ENTIRE PACKED `u64` -- 64 bits wide.

/// Bit offset for the local zone X (`x & 0x7`) within the packed `u64`.
const LOCAL_X_SHIFT: u32 = 0;
/// Bit offset for the local zone Z (`z & 0x7`) within the packed `u64`.
const LOCAL_Z_SHIFT: u32 = 3;
/// Bit offset for the width field within the packed `u64`.
const WIDTH_SHIFT: u32 = 6;
/// Bit offset for the length field within the packed `u64`.
const LENGTH_SHIFT: u32 = 12;
/// Bit offset for the lifetime flag within the packed `u64`.
const LIFETIME_SHIFT: u32 = 18;
/// Bit offset for the shared shape within the packed `u64`.
const SHAPE_SHIFT: u32 = 19;
/// Bit offset for the base (original) info within the packed `u64`.
const BASE_INFO_SHIFT: u32 = 24;
/// Bit offset for the current (possibly modified) info within the packed `u64`.
const CURRENT_INFO_SHIFT: u32 = 44;

/// Mask for the 3-bit local-coordinate fields.
const COORD_MASK: u64 = 0x7;
/// Mask for the 6-bit width and length fields.
const SIZE_MASK: u64 = 0x3F;
/// Mask for the single-bit lifetime field.
const LIFETIME_MASK: u64 = 0x1;
/// Mask for the 5-bit shared shape field.
const SHAPE_MASK: u64 = 0x1F;
/// Mask for the 20-bit info field (id, angle, blockwalk, blockrange).
const INFO_MASK: u64 = 0xFFFFF;

/// ---- THE BELOW SECTION IS FOR BUILDING THE INDIVIDUAL INFO BITS `u32` -- 20 bits wide.

/// Bit offset for the loc type id within the info word.
const ID_SHIFT: u32 = 0;
/// Bit offset for the angle within the info word.
const ANGLE_SHIFT: u32 = 16;
/// Bit offset for the blockwalk flag within the info word.
const BLOCKWALK_SHIFT: u32 = 18;
/// Bit offset for the blockrange flag within the info word.
const BLOCKRANGE_SHIFT: u32 = 19;

/// Mask for the 16-bit loc type id.
const ID_MASK: u32 = 0xFFFF;
/// Mask for the 2-bit angle.
const ANGLE_MASK: u32 = 0x3;
/// Mask for the single-bit blockwalk flag.
const BLOCKWALK_MASK: u32 = 0x1;
/// Mask for the single-bit blockrange flag.
const BLOCKRANGE_MASK: u32 = 0x1;

/// A placed location (scenery/object) in the game world.
///
/// All fixed fields are bit-packed into a single `u64`. The position is stored as the
/// *local* offset within the owning 8x8 zone (`x & 0x7`, `z & 0x7`);
/// the full world coordinate is reconstructed from the
/// zone's base via [`world_coord`](Self::world_coord). Width and length are
/// stored in 6 bits each, clamped to `0..=63`. The struct tracks both base and
/// current info to support runtime changes (e.g. opening a door changes its
/// `id`) while preserving the original state for revert on respawn-type locs.
///
/// The collision layer is *not* stored: it is a pure function of the shape
/// (see [`LocShape::layer`]) and is derived on demand by [`layer`](Self::layer).
///
/// Shape is stored *once* (shared by base and current): a runtime change never
/// alters a loc's shape. Angle, id and the `blockwalk`/`blockrange` flags *can*
/// change (e.g. opening a door rotates it), so they live per-info in both base and
/// current. Sharing the 5-bit shape frees the room for `width`/`length` to widen
/// to 6 bits each, and the flags stay packed so collision applies straight from
/// the loc without a cache lookup.
///
/// Layout (bit offsets):
/// - `0..3`   local X (`x & 0x7`)
/// - `3..6`   local Z (`z & 0x7`)
/// - `6..12`  width   (`0..=63`)
/// - `12..18` length  (`0..=63`)
/// - `18`     lifetime
/// - `19..24` shape   (shared `LocShape`)
/// - `24..44` base info    (`id16 | angle2 | blockwalk1 | blockrange1`)
/// - `44..64` current info (`id16 | angle2 | blockwalk1 | blockrange1`)
#[derive(Debug, Clone, Copy)]
pub struct Loc {
    packed: u64,
    pub last_clock: u64,
}

impl Loc {
    /// Creates a new `Loc` with the given position, dimensions, lifetime, and type info.
    ///
    /// Only the loc's position *within its 8x8 zone* (`coord.x() & 0x7`,
    /// `coord.z() & 0x7`) is stored; the level and zone base are recovered from the
    /// owning zone. Both the base info and current info are initialized to the
    /// same values, and `last_clock` is set to `u64::MAX` (not yet modified).
    ///
    /// # Arguments
    /// * `coord` - Grid coordinate where this loc is placed (only the intra-zone offset is kept).
    /// * `width` - Width of the loc in tiles (x-axis); clamped to `0..=63`.
    /// * `length` - Length of the loc in tiles (z-axis); clamped to `0..=63`.
    /// * `lifetime` - Whether this loc respawns (map-loaded) or despawns (runtime-spawned).
    /// * `id` - The loc type identifier from the config.
    /// * `shape` - The rendering shape of the loc (e.g., wall, centrepiece).
    /// * `angle` - The rotation angle of the loc.
    /// * `blockwalk` - Whether the loc blocks movement (from its loc type).
    /// * `blockrange` - Whether the loc blocks ranged projectiles (from its loc type).
    ///
    /// # Returns
    /// A new `Loc` with all fields bit-packed.
    #[inline(always)]
    pub const fn new(
        coord: CoordGrid,
        width: u8,
        length: u8,
        lifetime: EntityLifeTime,
        id: u16,
        shape: LocShape,
        angle: LocAngle,
        blockwalk: bool,
        blockrange: bool,
    ) -> Self {
        let info = Self::pack_info(id, angle, blockwalk, blockrange) as u64;
        let width = if width > SIZE_MASK as u8 {
            SIZE_MASK as u8
        } else {
            width
        };
        let length = if length > SIZE_MASK as u8 {
            SIZE_MASK as u8
        } else {
            length
        };
        let packed = (((coord.x() & COORD_MASK as u16) as u64) << LOCAL_X_SHIFT)
            | (((coord.z() & COORD_MASK as u16) as u64) << LOCAL_Z_SHIFT)
            | ((width as u64) << WIDTH_SHIFT)
            | ((length as u64) << LENGTH_SHIFT)
            | ((lifetime as u64) << LIFETIME_SHIFT)
            | ((shape as u64 & SHAPE_MASK) << SHAPE_SHIFT)
            | (info << BASE_INFO_SHIFT)
            | (info << CURRENT_INFO_SHIFT);
        Self {
            packed,
            last_clock: u64::MAX,
        }
    }

    /// Returns whether this loc should be visible to players.
    ///
    /// Despawn-type locs are always visible (they exist until removed). Respawn-type
    /// locs are visible when they have been changed from their base state or have
    /// never had their clock set (i.e., freshly loaded from the map).
    ///
    /// # Returns
    /// `true` if the loc should be sent to nearby players.
    #[inline(always)]
    pub const fn visible(&self) -> bool {
        match self.lifetime() {
            EntityLifeTime::Despawn => true,
            EntityLifeTime::Respawn => self.is_changed() || self.last_clock == u64::MAX,
        }
    }

    /// Returns the loc's local X offset (`0..=7`) within its owning zone.
    #[inline(always)]
    pub const fn local_x(&self) -> u8 {
        ((self.packed >> LOCAL_X_SHIFT) & COORD_MASK) as u8
    }

    /// Returns the loc's local Z offset (`0..=7`) within its owning zone.
    #[inline(always)]
    pub const fn local_z(&self) -> u8 {
        ((self.packed >> LOCAL_Z_SHIFT) & COORD_MASK) as u8
    }

    /// Returns `true` if this loc occupies world tile (`x`, `z`) within its
    /// owning zone -- i.e. their zone-local offsets match. Meant for searches
    /// scoped to the loc's own zone.
    #[inline(always)]
    pub const fn is_at(&self, x: u16, z: u16) -> bool {
        CoordGrid::local_eq(self.local_x(), self.local_z(), x, z)
    }

    /// Reconstructs the loc's full world coordinate from its owning zone's base.
    ///
    /// The loc only stores its intra-zone offset, so the zone's base tile (a
    /// multiple of 8) and level must be supplied -- typically `zone.coord`.
    #[inline(always)]
    pub const fn world_coord(&self, zone: ZoneCoordGrid) -> CoordGrid {
        CoordGrid::new(
            zone.x() | self.local_x() as u16,
            zone.y(),
            zone.z() | self.local_z() as u16,
        )
    }

    /// Returns the width of the loc in tiles along the x-axis (`0..=63`).
    #[inline(always)]
    pub const fn width(&self) -> u8 {
        ((self.packed >> WIDTH_SHIFT) & SIZE_MASK) as u8
    }

    /// Returns the length of the loc in tiles along the z-axis (`0..=63`).
    #[inline(always)]
    pub const fn length(&self) -> u8 {
        ((self.packed >> LENGTH_SHIFT) & SIZE_MASK) as u8
    }

    /// Returns the lifetime type of this loc (respawn or despawn).
    #[inline(always)]
    pub const fn lifetime(&self) -> EntityLifeTime {
        if (self.packed >> LIFETIME_SHIFT) & LIFETIME_MASK == 0 {
            EntityLifeTime::Respawn
        } else {
            EntityLifeTime::Despawn
        }
    }

    /// Returns the current loc type identifier.
    ///
    /// This reflects the current info, which may differ from the base info if the
    /// loc has been changed at runtime.
    #[inline(always)]
    pub const fn id(&self) -> u16 {
        (self.current_info() & ID_MASK) as u16
    }

    /// Returns the rendering shape of this loc.
    ///
    /// Stored once and shared by base and current state; a runtime change never
    /// alters the shape.
    ///
    /// # Safety
    /// Uses `transmute` internally; the 5-bit value is assumed to be a valid `LocShape` discriminant.
    #[inline(always)]
    pub const fn shape(&self) -> LocShape {
        unsafe { std::mem::transmute(((self.packed >> SHAPE_SHIFT) & SHAPE_MASK) as u8) }
    }

    /// Returns the current rotation angle of this loc.
    ///
    /// Read from the current info, so it reflects a runtime change (e.g. a door
    /// rotating); after a [`revert`](Self::revert) it matches the base again.
    ///
    /// # Safety
    /// Uses `transmute` internally; the 2-bit value is assumed to be a valid `LocAngle` discriminant.
    #[inline(always)]
    pub const fn angle(&self) -> LocAngle {
        unsafe { std::mem::transmute(((self.current_info() >> ANGLE_SHIFT) & ANGLE_MASK) as u8) }
    }

    /// Returns whether this loc currently blocks movement.
    ///
    /// Read from the current info so it reflects any runtime change; after a
    /// [`revert`](Self::revert) it again matches the base loc type.
    #[inline(always)]
    pub const fn blockwalk(&self) -> bool {
        (self.current_info() >> BLOCKWALK_SHIFT) & BLOCKWALK_MASK == 1
    }

    /// Returns whether this loc currently blocks ranged projectiles.
    ///
    /// Read from the current info so it reflects any runtime change; after a
    /// [`revert`](Self::revert) it again matches the base loc type.
    #[inline(always)]
    pub const fn blockrange(&self) -> bool {
        (self.current_info() >> BLOCKRANGE_SHIFT) & BLOCKRANGE_MASK == 1
    }

    /// Returns the collision layer of this loc.
    ///
    /// The layer is derived from the (shared) shape ([`LocShape::layer`]) rather
    /// than stored: it is a pure function of the shape, which never changes at
    /// runtime.
    #[inline(always)]
    pub const fn layer(&self) -> LocLayer {
        self.shape().layer()
    }

    /// Returns `true` if the current info differs from the base info.
    ///
    /// A changed loc needs to be communicated to clients so they see the updated
    /// type or angle. Respawn-type locs use this to determine visibility. (Shape is
    /// shared and never changes, so only the `id`, angle or flags can differ.)
    #[inline(always)]
    pub const fn is_changed(&self) -> bool {
        self.current_info() != self.base_info()
    }

    /// Modifies the current info of this loc to a new type id, angle and flags.
    ///
    /// The base info is left unchanged, allowing the loc to be reverted later.
    /// This is used for runtime loc changes such as opening/closing doors (which
    /// can rotate the loc). The shape is shared with the base state and is
    /// intentionally *not* changed.
    ///
    /// # Arguments
    /// * `id` - New loc type identifier.
    /// * `angle` - New rotation angle.
    /// * `blockwalk` - Whether the new loc type blocks movement.
    /// * `blockrange` - Whether the new loc type blocks ranged projectiles.
    ///
    /// # Side Effects
    /// * Overwrites the current info bits in `self.packed`.
    #[inline(always)]
    pub const fn change(&mut self, id: u16, angle: LocAngle, blockwalk: bool, blockrange: bool) {
        let info = Self::pack_info(id, angle, blockwalk, blockrange) as u64;
        self.packed =
            (self.packed & !(INFO_MASK << CURRENT_INFO_SHIFT)) | (info << CURRENT_INFO_SHIFT);
    }

    /// Reverts the current info back to the base info.
    ///
    /// Used when a respawn-type loc's change timer expires, restoring it to its
    /// original map-loaded state (e.g., a door closing automatically).
    ///
    /// # Side Effects
    /// * Copies the base info bits into the current info bits in `self.packed`.
    #[inline(always)]
    pub const fn revert(&mut self) {
        let base = (self.packed >> BASE_INFO_SHIFT) & INFO_MASK;
        self.packed =
            (self.packed & !(INFO_MASK << CURRENT_INFO_SHIFT)) | (base << CURRENT_INFO_SHIFT);
    }

    /// Returns a unique local identifier for this loc within its zone.
    ///
    /// The identifier is composed of the local x and z coordinates (3 bits each)
    /// and the layer (2 bits), packed into a `u64`.
    #[inline(always)]
    pub const fn lid(&self) -> u64 {
        ((self.local_x() as u64) << LOCAL_X_SHIFT)
            | ((self.local_z() as u64) << LOCAL_Z_SHIFT)
            | ((self.layer() as u64 & 0x3) << 6)
    }

    /// Returns the local zone-relative coordinate packed into a single byte.
    ///
    /// The upper 4 bits hold `x & 0x7` and the lower 4 bits hold `z & 0x7`, suitable
    /// for encoding in network packets.
    #[inline(always)]
    pub const fn packed_zone_coord(&self) -> u8 {
        CoordGrid::packed_zone_coord(self.local_x() as u16, self.local_z() as u16)
    }

    /// Returns the shape and angle packed into a single byte for network encoding.
    ///
    /// The upper 6 bits hold the shape (shifted left by 2) and the lower 2 bits hold
    /// the angle.
    #[inline(always)]
    pub const fn packed_shape_angle(&self) -> u8 {
        ((self.shape() as u8) << 2) | (self.angle() as u8)
    }

    /// Packs a loc type id, angle and collision flags into a 20-bit info value.
    ///
    /// Layout: bits 0-15 = type_id, bits 16-17 = angle, bit 18 = blockwalk,
    /// bit 19 = blockrange.
    #[inline(always)]
    const fn pack_info(type_id: u16, angle: LocAngle, blockwalk: bool, blockrange: bool) -> u32 {
        ((type_id as u32 & ID_MASK) << ID_SHIFT)
            | ((angle as u32 & ANGLE_MASK) << ANGLE_SHIFT)
            | ((blockwalk as u32 & BLOCKWALK_MASK) << BLOCKWALK_SHIFT)
            | ((blockrange as u32 & BLOCKRANGE_MASK) << BLOCKRANGE_SHIFT)
    }

    /// Extracts the base (original) info from the packed representation.
    #[inline(always)]
    const fn base_info(&self) -> u32 {
        ((self.packed >> BASE_INFO_SHIFT) & INFO_MASK) as u32
    }

    /// Extracts the current (possibly modified) info from the packed representation.
    #[inline(always)]
    const fn current_info(&self) -> u32 {
        ((self.packed >> CURRENT_INFO_SHIFT) & INFO_MASK) as u32
    }
}
