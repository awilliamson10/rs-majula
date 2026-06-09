use crate::lifetime::EntityLifeTime;
use rs_grid::{CoordGrid, ZoneCoordGrid};

/// Number of ticks after which a private ground object becomes visible to all players.
pub const REVEAL_TICKS: u64 = 100;
/// Sentinel value indicating that a ground object has no specific receiver (visible to all).
pub const NO_RECEIVER: u64 = u64::MAX;

/// ---- THE BELOW SECTION IS FOR BUILDING THE ENTIRE PACKED `u64` -- 62 bits wide.

/// Bit offset for the local zone X (`x & 0x7`) within the packed `u64`.
const LOCAL_X_SHIFT: u32 = 0;
/// Bit offset for the local zone Z (`z & 0x7`) within the packed `u64`.
const LOCAL_Z_SHIFT: u32 = 3;
/// Bit offset for the lifetime flag within the packed `u64`.
const LIFETIME_SHIFT: u32 = 6;
/// Bit offset for the obj type id within the packed `u64`.
const ID_SHIFT: u32 = 7;
/// Bit offset for the stack count within the packed `u64`.
const COUNT_SHIFT: u32 = 23;
/// Bit offset for the per-(tile, id) instance slot within the packed `u64`.
const SLOT_SHIFT: u32 = 54;

/// Mask for the 3-bit local-coordinate fields.
const COORD_MASK: u64 = 0x7;
/// Mask for the single-bit lifetime field.
const LIFETIME_MASK: u64 = 0x1;
/// Mask for the 16-bit obj type id.
const ID_MASK: u64 = 0xFFFF;
/// Mask for the 31-bit stack count.
const COUNT_MASK: u64 = 0x7FFFFFFF;
/// Mask for the 8-bit instance slot.
const SLOT_MASK: u64 = 0xFF;

/// A ground object (item on the floor) in the game world.
///
/// The core fields (position, lifetime, id, count) are bit-packed into a single
/// `u64`. To keep the struct compact, the position is stored as the *local*
/// offset within the owning 8x8 zone (`x & 0x7`, `z & 0x7`); the full world
/// coordinate is reconstructed from the zone's base via
/// [`world_coord`](Self::world_coord). The remaining fields track the player who
/// can see it privately (receiver), the tick at which it becomes public
/// (reveal), and the tick at which it was last modified or scheduled for removal.
///
/// Packed layout (bit offsets):
/// - `0..3`   local X (`x & 0x7`)
/// - `3..6`   local Z (`z & 0x7`)
/// - `6`      lifetime
/// - `7..23`  obj type id (16 bits)
/// - `23..54` stack count (31 bits)
/// - `54..62` instance slot (8 bits)
/// - `62..64` unused
#[derive(Debug, Clone)]
pub struct Obj {
    packed: u64,
    pub receiver37: u64,
    pub reveal: u64,
    pub last_clock: u64,
}

impl Obj {
    /// Creates a new ground object at the given coordinate.
    ///
    /// Only the object's position *within its 8x8 zone* is stored; the level and
    /// zone base are recovered from the owning zone. The object starts with no
    /// receiver (visible to all), no reveal tick, and no last-clock, meaning it
    /// has not yet been scheduled for any state transition.
    ///
    /// # Arguments
    /// * `coord` - Grid coordinate where this object is placed (only the intra-zone offset is kept).
    /// * `lifetime` - Whether this object respawns (map-loaded) or despawns (runtime-dropped).
    /// * `id` - The obj type identifier from the config.
    /// * `count` - The stack count of the item.
    ///
    /// # Returns
    /// A new `Obj` with core fields bit-packed and metadata set to sentinel values.
    #[inline(always)]
    pub const fn new(coord: CoordGrid, lifetime: EntityLifeTime, id: u16, count: u32) -> Self {
        let packed = (((coord.x() & COORD_MASK as u16) as u64) << LOCAL_X_SHIFT)
            | (((coord.z() & COORD_MASK as u16) as u64) << LOCAL_Z_SHIFT)
            | ((lifetime as u64) << LIFETIME_SHIFT)
            | ((id as u64) << ID_SHIFT)
            | (((count as u64) & COUNT_MASK) << COUNT_SHIFT);
        Self {
            packed,
            receiver37: NO_RECEIVER,
            reveal: u64::MAX,
            last_clock: u64::MAX,
        }
    }

    /// Returns the obj's local X offset (`0..=7`) within its owning zone.
    #[inline(always)]
    pub const fn local_x(&self) -> u8 {
        ((self.packed >> LOCAL_X_SHIFT) & COORD_MASK) as u8
    }

    /// Returns the obj's local Z offset (`0..=7`) within its owning zone.
    #[inline(always)]
    pub const fn local_z(&self) -> u8 {
        ((self.packed >> LOCAL_Z_SHIFT) & COORD_MASK) as u8
    }

    /// Returns `true` if this obj occupies world tile (`x`, `z`) within its
    /// owning zone -- i.e. their zone-local offsets match. Meant for searches
    /// scoped to the obj's own zone.
    #[inline(always)]
    pub const fn is_at(&self, x: u16, z: u16) -> bool {
        CoordGrid::local_eq(self.local_x(), self.local_z(), x, z)
    }

    /// Reconstructs the obj's full world coordinate from its owning zone's base.
    ///
    /// The obj only stores its intra-zone offset, so the zone's base tile (a
    /// multiple of 8) and level must be supplied -- typically `zone.coord`.
    #[inline(always)]
    pub const fn world_coord(&self, zone: ZoneCoordGrid) -> CoordGrid {
        CoordGrid::new(
            zone.x() | self.local_x() as u16,
            zone.y(),
            zone.z() | self.local_z() as u16,
        )
    }

    /// Returns the lifetime type of this object (respawn or despawn).
    #[inline(always)]
    pub const fn lifetime(&self) -> EntityLifeTime {
        if (self.packed >> LIFETIME_SHIFT) & LIFETIME_MASK == 0 {
            EntityLifeTime::Respawn
        } else {
            EntityLifeTime::Despawn
        }
    }

    /// Returns the obj type identifier from the config.
    #[inline(always)]
    pub const fn id(&self) -> u16 {
        ((self.packed >> ID_SHIFT) & ID_MASK) as u16
    }

    /// Returns the stack count of this object.
    #[inline(always)]
    pub const fn count(&self) -> u32 {
        ((self.packed >> COUNT_SHIFT) & COUNT_MASK) as u32
    }

    /// Sets the stack count of this object.
    ///
    /// Used when stackable drops accumulate onto an existing floor stack.
    #[inline(always)]
    pub const fn set_count(&mut self, count: u32) {
        self.packed = (self.packed & !(COUNT_MASK << COUNT_SHIFT))
            | (((count as u64) & COUNT_MASK) << COUNT_SHIFT);
    }

    /// Returns this obj's instance slot.
    ///
    /// The slot disambiguates objs that share the same tile and type id within a
    /// zone (two identical non-stackable drops, or two players' private stacks of
    /// the same item), so [`oid`](Self::oid) is unique per obj within its zone. The
    /// zone assigns it when the obj is inserted; it is `0` until then and never
    /// changes afterward.
    #[inline(always)]
    pub const fn slot(&self) -> u8 {
        ((self.packed >> SLOT_SHIFT) & SLOT_MASK) as u8
    }

    /// Sets this obj's instance slot.
    ///
    /// Called by the zone when the obj is inserted, to give it a slot unused by
    /// any other obj sharing its tile and id. See [`slot`](Self::slot).
    #[inline(always)]
    pub const fn set_slot(&mut self, slot: u8) {
        self.packed = (self.packed & !(SLOT_MASK << SLOT_SHIFT))
            | (((slot as u64) & SLOT_MASK) << SLOT_SHIFT);
    }

    /// Returns whether this object should be visible at the given engine tick.
    ///
    /// Objects with no pending state change (`last_clock == u64::MAX`) are always
    /// visible. Despawn-type objects are visible while the clock has not yet reached
    /// their `last_clock`. Respawn-type objects become visible once the clock reaches
    /// or exceeds their `last_clock`.
    ///
    /// # Arguments
    /// * `clock` - The current engine tick.
    #[inline(always)]
    pub const fn visible(&self, clock: u64) -> bool {
        if self.last_clock == u64::MAX {
            return true;
        }
        match self.lifetime() {
            EntityLifeTime::Despawn => clock < self.last_clock,
            EntityLifeTime::Respawn => clock >= self.last_clock,
        }
    }

    /// Returns a unique identifier for this object within its zone.
    ///
    /// Composed of the local x and z coordinates (3 bits each), the obj type id
    /// (16 bits), and the instance [`slot`](Self::slot) (8 bits), packed into a
    /// `u64` (30 bits used). Because the zone gives each obj the lowest free slot
    /// among objs sharing its tile and id, every obj in a zone -- stackable or not,
    /// public or private -- gets a distinct oid. Used as the entity key for zone
    /// event dedup/cancellation.
    #[inline(always)]
    pub const fn oid(&self) -> u64 {
        ((self.local_x() as u64) << LOCAL_X_SHIFT)
            | ((self.local_z() as u64) << LOCAL_Z_SHIFT)
            | ((self.id() as u64) << 6)
            | ((self.slot() as u64) << 22)
    }

    /// Returns the local zone-relative coordinate packed into a single byte.
    ///
    /// The upper 4 bits hold `x & 0x7` and the lower 4 bits hold `z & 0x7`, suitable
    /// for encoding in network packets.
    #[inline(always)]
    pub const fn packed_zone_coord(&self) -> u8 {
        CoordGrid::packed_zone_coord(self.local_x() as u16, self.local_z() as u16)
    }
}
