use crate::lifetime::EntityLifeTime;
use rs_grid::CoordGrid;

/// Number of ticks after which a private ground object becomes visible to all players.
pub const REVEAL_TICKS: u64 = 100;
/// Sentinel value indicating that a ground object has no specific receiver (visible to all).
pub const NO_RECEIVER: u64 = u64::MAX;

/// Mask for extracting the coordinate from the packed `u64` (lower 32 bits).
const COORD_MASK: u64 = 0xFFFF_FFFF;
/// Bit offset for the lifecycle flag within the packed `u64`.
const LIFECYCLE_SHIFT: u32 = 32;
/// Bit offset for the obj type id within the packed `u64`.
const ID_SHIFT: u32 = 33;

/// A ground object (item on the floor) in the game world.
///
/// Core fields (coord, lifecycle, id) are bit-packed into a single `u64`.
/// Additional fields track the item count, the player who can see it privately
/// (receiver), the tick at which it becomes public (reveal), and the tick at
/// which it was last modified or scheduled for removal.
#[derive(Debug, Clone)]
pub struct Obj {
    packed: u64,
    pub count: u32,
    pub receiver37: u64,
    pub reveal: u64,
    pub last_clock: u64,
}

impl Obj {
    /// Creates a new ground object at the given coordinate.
    ///
    /// The object starts with no receiver (visible to all), no reveal tick, and no
    /// last-clock, meaning it has not yet been scheduled for any state transition.
    ///
    /// # Arguments
    /// * `coord` - Grid coordinate where this object is placed.
    /// * `lifecycle` - Whether this object respawns (map-loaded) or despawns (runtime-dropped).
    /// * `id` - The obj type identifier from the config.
    /// * `count` - The stack count of the item.
    ///
    /// # Returns
    /// A new `Obj` with core fields bit-packed and metadata set to sentinel values.
    #[inline(always)]
    pub const fn new(coord: CoordGrid, lifecycle: EntityLifeTime, id: u16, count: u32) -> Self {
        let packed = (coord.packed() as u64)
            | ((lifecycle as u64) << LIFECYCLE_SHIFT)
            | ((id as u64) << ID_SHIFT);
        Self {
            packed,
            count,
            receiver37: NO_RECEIVER,
            reveal: u64::MAX,
            last_clock: u64::MAX,
        }
    }

    /// Returns the grid coordinate where this object is placed.
    #[inline(always)]
    pub const fn coord(&self) -> CoordGrid {
        CoordGrid::from((self.packed & COORD_MASK) as u32)
    }

    /// Returns the lifecycle type of this object (respawn or despawn).
    #[inline(always)]
    pub const fn lifetime(&self) -> EntityLifeTime {
        if (self.packed >> LIFECYCLE_SHIFT) & 1 == 0 {
            EntityLifeTime::Respawn
        } else {
            EntityLifeTime::Despawn
        }
    }

    /// Returns the obj type identifier from the config.
    #[inline(always)]
    pub const fn id(&self) -> u16 {
        (self.packed >> ID_SHIFT) as u16
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

    /// Returns a unique local identifier for this object within its zone.
    ///
    /// The identifier is composed of the local x and z coordinates (3 bits each),
    /// the obj type id (16 bits), and the receiver (lower 32 bits, or 0 if no
    /// receiver), packed into a `u64`. Used for deduplication when tracking which
    /// objects a player has already seen.
    #[inline(always)]
    pub fn oid(&self) -> u64 {
        let coord = ((self.coord().x() & 7) as u64) | (((self.coord().z() & 7) as u64) << 3);
        let r = if self.receiver37 == NO_RECEIVER {
            0
        } else {
            self.receiver37
        };
        coord | ((self.id() as u64) << 6) | ((r & 0xFFFF_FFFF) << 22)
    }
}
