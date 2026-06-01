use crate::zone::Zone;
use rs_grid::ZoneCoordGrid;
use rustc_hash::FxHashMap;

/// Global lookup table mapping zone coordinates to their [`Zone`] instances.
///
/// Stored in the `Engine` struct, `ZoneMap` provides O(1) access to any zone
/// by its (x, y, z) coordinate triple. Zones are lazily created on first
/// mutable access via [`zone_mut`](Self::zone_mut).
///
/// The `Zone` values are boxed so the hash map's `(key, value)` storage holds
/// only a small pointer per slot (~12 B) instead of a full inline `Zone`
/// (~170 B). With thousands of loaded zones this keeps the probe array
/// cache-resident, so the per-zone lookups in the info phase
/// (`get_nearby_*`, `update_zones`) stay cheap.
pub struct ZoneMap {
    pub zones: FxHashMap<ZoneCoordGrid, Box<Zone>>,
}

impl ZoneMap {
    /// Creates a new, empty `ZoneMap`.
    ///
    /// # Returns
    ///
    /// A `ZoneMap` with no zones allocated. Zones are created lazily via
    /// [`zone_mut`](Self::zone_mut).
    ///
    /// **Called by:** `Engine::new` during server startup.
    #[inline]
    #[allow(clippy::new_without_default)]
    pub fn new() -> ZoneMap {
        ZoneMap {
            zones: FxHashMap::default(),
        }
    }

    /// Returns an immutable reference to the zone at the given coordinates, if it exists.
    ///
    /// # Arguments
    ///
    /// * `x` -- The zone x coordinate.
    /// * `y` -- The zone y (level/plane) coordinate.
    /// * `z` -- The zone z coordinate.
    ///
    /// # Returns
    ///
    /// `Some(&Zone)` if the zone has been previously created, `None` otherwise.
    ///
    /// **Called by:** `Engine` methods, `ActivePlayer::update_zones`,
    /// `BuildArea` neighbour scanning, `InfoProtocol` queries, op handlers.
    #[inline]
    pub fn zone(&self, x: u16, y: u8, z: u16) -> Option<&Zone> {
        let coord = ZoneCoordGrid::new(x, y, z);
        self.zones.get(&coord).map(|z| &**z)
    }

    /// Returns a mutable reference to the zone at the given coordinates, creating it if absent.
    ///
    /// This is the primary entry point for zone mutation. If no zone exists at
    /// the given coordinates, a new empty [`Zone`] is inserted and returned.
    ///
    /// # Arguments
    ///
    /// * `x` -- The zone x coordinate.
    /// * `y` -- The zone y (level/plane) coordinate.
    /// * `z` -- The zone z coordinate.
    ///
    /// # Returns
    ///
    /// A mutable reference to the (possibly newly created) `Zone`.
    ///
    /// **Called by:** `Engine` methods for entity placement, `GameMap::load` for
    /// static loc/obj population, `ActivePlayer::update_zones`, zone phase processing.
    #[inline]
    pub fn zone_mut(&mut self, x: u16, y: u8, z: u16) -> &mut Zone {
        let coord = ZoneCoordGrid::new(x, y, z);
        // `or_insert_with` so the `Zone` is only constructed on actual insert
        // (the eager `or_insert` built one every call even when present).
        self.zones
            .entry(coord)
            .or_insert_with(|| Box::new(Zone::new(coord)))
    }
}
