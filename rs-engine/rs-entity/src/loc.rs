use crate::lifetime::EntityLifeTime;
use rs_grid::CoordGrid;
use rs_pack::types::{LocAngle, LocLayer, LocShape};

/// Bit offset for the width field within the packed `u128`.
const WIDTH_SHIFT: u32 = 32;
/// Bit offset for the length field within the packed `u128`.
const LENGTH_SHIFT: u32 = 40;
/// Bit offset for the lifecycle flag within the packed `u128`.
const LIFECYCLE_SHIFT: u32 = 48;
/// Bit offset for the base (original) info within the packed `u128`.
const BASE_INFO_SHIFT: u32 = 49;
/// Bit offset for the current (possibly modified) info within the packed `u128`.
const CURRENT_INFO_SHIFT: u32 = 74;

/// Mask for extracting info fields (id, shape, angle, layer) -- 25 bits wide.
const INFO_MASK: u128 = 0x1FFFFFF; // 25 bits

/// A placed location (scenery/object) in the game world.
///
/// All fixed fields (coord, width, length, lifecycle, base info, current info) are
/// bit-packed into a single `u128` for compact storage. The struct tracks both
/// base and current info to support runtime changes (e.g., opening a door changes
/// its shape) while preserving the original state for revert on respawn-type locs.
#[derive(Debug, Clone, Copy)]
pub struct Loc {
    packed: u128,
    pub last_clock: Option<u64>,
}

impl Loc {
    /// Creates a new `Loc` with the given position, dimensions, lifecycle, and type info.
    ///
    /// Both the base info and current info are initialized to the same values. The
    /// `last_clock` is set to `None`, meaning the loc has not been modified since creation.
    ///
    /// # Arguments
    /// * `coord` - Grid coordinate where this loc is placed.
    /// * `width` - Width of the loc in tiles (x-axis).
    /// * `length` - Length of the loc in tiles (z-axis).
    /// * `lifecycle` - Whether this loc respawns (map-loaded) or despawns (runtime-spawned).
    /// * `id` - The loc type identifier from the config.
    /// * `shape` - The rendering shape of the loc (e.g., wall, centrepiece).
    /// * `angle` - The rotation angle of the loc.
    /// * `layer` - The collision layer the loc occupies.
    ///
    /// # Returns
    /// A new `Loc` with all fields bit-packed.
    #[inline(always)]
    pub const fn new(
        coord: CoordGrid,
        width: u8,
        length: u8,
        lifecycle: EntityLifeTime,
        id: u16,
        shape: LocShape,
        angle: LocAngle,
        layer: LocLayer,
    ) -> Self {
        let info = Self::pack_info(id, shape, angle, layer);
        let packed = (coord.packed() as u128)
            | ((width as u128) << WIDTH_SHIFT)
            | ((length as u128) << LENGTH_SHIFT)
            | ((lifecycle as u128) << LIFECYCLE_SHIFT)
            | ((info as u128) << BASE_INFO_SHIFT)
            | ((info as u128) << CURRENT_INFO_SHIFT);
        Self {
            packed,
            last_clock: None,
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
            EntityLifeTime::Respawn => self.is_changed() || self.last_clock.is_none(),
        }
    }

    /// Returns the grid coordinate where this loc is placed.
    #[inline(always)]
    pub const fn coord(&self) -> CoordGrid {
        CoordGrid::from(self.packed as u32)
    }

    /// Returns the width of the loc in tiles along the x-axis.
    #[inline(always)]
    pub const fn width(&self) -> u8 {
        (self.packed >> WIDTH_SHIFT) as u8
    }

    /// Returns the length of the loc in tiles along the z-axis.
    #[inline(always)]
    pub const fn length(&self) -> u8 {
        (self.packed >> LENGTH_SHIFT) as u8
    }

    /// Returns the lifecycle type of this loc (respawn or despawn).
    #[inline(always)]
    pub const fn lifetime(&self) -> EntityLifeTime {
        if (self.packed >> LIFECYCLE_SHIFT) & 1 == 0 {
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
        (self.current_info() & 0xFFFF) as u16
    }

    /// Returns the current rendering shape of this loc.
    ///
    /// # Safety
    /// Uses `transmute` internally; the 5-bit value is assumed to be a valid `LocShape` discriminant.
    #[inline(always)]
    pub const fn shape(&self) -> LocShape {
        unsafe { std::mem::transmute(((self.current_info() >> 16) & 0x1F) as u8) }
    }

    /// Returns the current rotation angle of this loc.
    ///
    /// # Safety
    /// Uses `transmute` internally; the 2-bit value is assumed to be a valid `LocAngle` discriminant.
    #[inline(always)]
    pub const fn angle(&self) -> LocAngle {
        unsafe { std::mem::transmute(((self.current_info() >> 21) & 0x3) as u8) }
    }

    /// Returns the collision layer of this loc, read from the base info.
    ///
    /// The layer is always taken from the base info (not current) because the
    /// collision layer does not change when a loc is modified at runtime.
    ///
    /// # Safety
    /// Uses `transmute` internally; the 2-bit value is assumed to be a valid `LocLayer` discriminant.
    #[inline(always)]
    pub const fn layer(&self) -> LocLayer {
        unsafe { std::mem::transmute(((self.base_info() >> 23) & 0x3) as u8) }
    }

    /// Returns `true` if the current info differs from the base info.
    ///
    /// A changed loc needs to be communicated to clients so they see the updated
    /// shape, angle, or type. Respawn-type locs use this to determine visibility.
    #[inline(always)]
    pub const fn is_changed(&self) -> bool {
        self.current_info() != self.base_info()
    }

    /// Modifies the current info of this loc to a new type, shape, angle, and layer.
    ///
    /// The base info is left unchanged, allowing the loc to be reverted later.
    /// This is used for runtime loc changes such as opening/closing doors.
    ///
    /// # Arguments
    /// * `id` - New loc type identifier.
    /// * `shape` - New rendering shape.
    /// * `angle` - New rotation angle.
    /// * `layer` - New collision layer.
    ///
    /// # Side Effects
    /// * Overwrites the current info bits in `self.packed`.
    #[inline(always)]
    pub const fn change(&mut self, id: u16, shape: LocShape, angle: LocAngle, layer: LocLayer) {
        let info = Self::pack_info(id, shape, angle, layer) as u128;
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
    /// The identifier is composed of the local x and z coordinates (3 bits each,
    /// masked to `& 7`) and the layer (2 bits), packed into a `u64`.
    #[inline(always)]
    pub const fn lid(&self) -> u64 {
        let coord = ((self.coord().x() & 7) as u64) | (((self.coord().z() & 7) as u64) << 3);
        coord | ((self.layer() as u64 & 0x3) << 6)
    }

    /// Returns the local zone-relative coordinate packed into a single byte.
    ///
    /// The upper 4 bits hold `x & 7` and the lower 4 bits hold `z & 7`, suitable
    /// for encoding in network packets.
    #[inline(always)]
    pub const fn packed_zone_coord(&self) -> u8 {
        ((self.coord().x() & 7) << 4) as u8 | (self.coord().z() & 7) as u8
    }

    /// Returns the shape and angle packed into a single byte for network encoding.
    ///
    /// The upper 6 bits hold the shape (shifted left by 2) and the lower 2 bits hold
    /// the angle.
    #[inline(always)]
    pub const fn packed_shape_angle(&self) -> u8 {
        ((self.shape() as u8) << 2) | (self.angle() as u8 & 3)
    }

    /// Packs a loc type id, shape, angle, and layer into a 25-bit value.
    ///
    /// Layout: bits 0-15 = type_id, bits 16-20 = shape, bits 21-22 = angle, bits 23-24 = layer.
    #[inline(always)]
    const fn pack_info(type_id: u16, shape: LocShape, angle: LocAngle, layer: LocLayer) -> u32 {
        (type_id as u32 & 0xFFFF)
            | ((shape as u32 & 0x1F) << 16)
            | ((angle as u32 & 0x3) << 21)
            | ((layer as u32 & 0x3) << 23)
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
