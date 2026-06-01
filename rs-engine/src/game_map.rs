use crate::active_npc::ActiveNpc;
use num_enum::TryFromPrimitive;
use rs_entity::{EntityLifeTime, Loc, Obj};
use rs_grid::CoordGrid;
use rs_grid::mapsquare_coord::MapsquareCoordGrid;
use rs_io::Packet;
use rs_io::bz2::bz2_decompress;
use rs_pack::cache::CacheStore;
use rs_pack::types::{LocAngle, LocLayer, LocShape};
use rs_var::VarSet;
use rs_zone::zone_map::ZoneMap;

/// Mapsquare width in tiles.
const X: usize = 64;
/// Number of vertical levels (floors).
const Y: usize = 4;
/// Mapsquare height in tiles.
const Z: usize = 64;
/// Total number of tiles per mapsquare (X * Y * Z).
const MAPSQUARE: usize = X * Y * Z;

/// Tile flag: no special flags set.
const OPEN: u8 = 0x0;
/// Tile flag: tile is blocked (floor collision).
const BLOCK_MAP_SQUARE: u8 = 0x1;
/// Tile flag: entities on this tile are rendered one level below (bridge).
const LINK_BELOW: u8 = 0x2;
/// Tile flag: roofs above this tile are removed when viewing.
const REMOVE_ROOFS: u8 = 0x4;
/// Tile flag: the tile below is visible through the floor.
#[allow(unused)]
const VISIBLE_BELOW: u8 = 0x8;
/// Tile flag: the tile is not included in low-detail rendering.
#[allow(unused)]
const NOT_LOW_DETAIL: u8 = 0x10;

/// Provides static methods for loading the world map data (ground,
/// locations, NPCs, and objects) from the game cache into the collision
/// system and zone map.
pub struct GameMap;

/// Decompresses a BZip2-compressed map data blob prefixed with a 4-byte
/// big-endian uncompressed size.
///
/// # Arguments
/// * `data` - The raw compressed data, where bytes 0..4 are the decompressed
///   size and bytes 4.. are the BZip2 payload.
///
/// # Returns
/// A [`Packet`] wrapping the decompressed bytes.
fn decompress_map(data: &[u8]) -> Packet {
    let size = ((data[0] as usize) << 24)
        | ((data[1] as usize) << 16)
        | ((data[2] as usize) << 8)
        | (data[3] as usize);
    Packet::from(bz2_decompress(&data[4..], size, true, 0))
}

impl GameMap {
    /// Loads the entire world map from the cache, populating collision data,
    /// static locations, NPC spawns, and ground objects.
    ///
    /// Iterates over every mapsquare key in the cache and loads each of the
    /// four data layers: ground tiles (`m`), locations (`l`), NPCs (`n`),
    /// and objects (`o`).
    ///
    /// # Arguments
    /// * `members` - Whether members-only content should be loaded.
    /// * `cache` - The game cache containing mapsquare data.
    /// * `zones` - The zone map to populate with static locs and objs.
    ///
    /// # Returns
    /// A `Vec<ActiveNpc>` of all NPC spawns loaded from the map data.
    ///
    /// # Side Effects
    /// * Allocates zones and sets collision flags via `rsmod`.
    /// * Populates static locs and objs in the zone map.
    ///
    /// # Call Stack
    /// **Calls:** [`load_ground`](Self::load_ground),
    /// [`load_locations`](Self::load_locations),
    /// [`load_npcs`](Self::load_npcs), [`load_objs`](Self::load_objs)
    pub fn load(members: bool, cache: &CacheStore, zones: &mut ZoneMap) -> Vec<ActiveNpc> {
        let mut spawned_npcs = Vec::new();
        let mapsquare_keys: Vec<(u8, u8)> = cache
            .mapsquares
            .keys()
            .filter(|(c, _, _)| *c == 'm')
            .map(|(_, x, z)| (*x, *z))
            .collect();

        for (mx, mz) in mapsquare_keys {
            let originx = (mx as u16) << 6;
            let originz = (mz as u16) << 6;

            let mut lands = [0u8; MAPSQUARE];

            if let Some(data) = cache.mapsquares.get(&('m', mx, mz)) {
                Self::load_ground(
                    members,
                    &mut lands,
                    &mut decompress_map(data),
                    cache,
                    originx,
                    originz,
                );
            }

            if let Some(data) = cache.mapsquares.get(&('l', mx, mz)) {
                Self::load_locations(
                    members,
                    &lands,
                    &mut decompress_map(data),
                    cache,
                    zones,
                    originx,
                    originz,
                );
            }

            if let Some(data) = cache.mapsquares.get(&('n', mx, mz)) {
                Self::load_npcs(
                    members,
                    &mut decompress_map(data),
                    cache,
                    &mut spawned_npcs,
                    originx,
                    originz,
                );
            }

            if let Some(data) = cache.mapsquares.get(&('o', mx, mz)) {
                Self::load_objs(
                    members,
                    &mut decompress_map(data),
                    cache,
                    zones,
                    originx,
                    originz,
                );
            }
        }

        spawned_npcs
    }

    /// Decodes ground tile flags from the mapsquare buffer and applies floor
    /// and roof collision data to the `rsmod` collision system.
    ///
    /// In the first pass, tile flags (overlay, underlay, height, and land
    /// flags) are read from the buffer. In the second pass, collision changes
    /// are applied: blocked tiles get floor collision, and tiles with the
    /// `LINK_BELOW` flag shift their collision down one level (bridge logic).
    ///
    /// On a free-to-play world, collision and zone allocation are skipped for
    /// tiles that are neither free-to-play nor bordering free-to-play land.
    ///
    /// # Arguments
    /// * `members` - Whether this world loads members-only areas.
    /// * `lands` - Output array to receive per-tile flags for the mapsquare.
    /// * `buf` - The decompressed mapsquare ground data.
    /// * `cache` - The game cache, used for free-to-play zone lookups.
    /// * `originx` - The mapsquare origin X in absolute tile coordinates.
    /// * `originz` - The mapsquare origin Z in absolute tile coordinates.
    ///
    /// # Side Effects
    /// * Allocates zones and sets floor/roof collision via `rsmod`.
    ///
    /// # Call Stack
    /// **Called by:** [`load`](Self::load)
    fn load_ground(
        members: bool,
        lands: &mut [u8; MAPSQUARE],
        buf: &mut Packet,
        cache: &CacheStore,
        originx: u16,
        originz: u16,
    ) {
        for y in 0..Y {
            for x in 0..X {
                for z in 0..Z {
                    let coord = MapsquareCoordGrid::new(x as u8, y as u8, z as u8);
                    loop {
                        let opcode = buf.g1();
                        if opcode == 0 {
                            break;
                        } else if opcode == 1 {
                            buf.pos += 1;
                            break;
                        }
                        if opcode <= 49 {
                            buf.pos += 1;
                        } else if opcode <= 81 {
                            lands[coord.packed() as usize] = opcode - 49;
                        }
                    }
                }
            }
        }

        for y in 0..Y {
            for x in 0..X {
                let coordx = originx + x as u16;
                for z in 0..Z {
                    let coord = MapsquareCoordGrid::new(x as u8, y as u8, z as u8);
                    let coordz = originz + z as u16;

                    if !members
                        && !cache.is_free(coordx, coordz)
                        && !cache.borders_free(coordx, coordz)
                    {
                        continue;
                    }

                    if x % 7 == 0 && z % 7 == 0 {
                        rsmod::allocate_if_absent(coordx, coordz, y as u8);
                    }

                    let land = lands[coord.packed() as usize];

                    if (land & REMOVE_ROOFS) != OPEN {
                        rsmod::change_roof(coordx, coordz, y as u8, true);
                    }

                    if (land & BLOCK_MAP_SQUARE) != BLOCK_MAP_SQUARE {
                        continue;
                    }

                    let bridged = if y == 1 {
                        (land & LINK_BELOW) == LINK_BELOW
                    } else {
                        let coord = MapsquareCoordGrid::new(x as u8, 1, z as u8);
                        (lands[coord.packed() as usize] & LINK_BELOW) == LINK_BELOW
                    };

                    let bridge = if bridged { y as i8 - 1 } else { y as i8 };
                    if bridge < 0 {
                        continue;
                    }

                    rsmod::change_floor(coordx, coordz, bridge as u8, true);
                }
            }
        }
    }

    /// Decodes location (loc) definitions from the mapsquare buffer, applies
    /// their collision to the `rsmod` system, and registers them as static
    /// locs in the zone map.
    ///
    /// Handles bridge-level adjustments using the `lands` tile flags. On a
    /// free-to-play world, locs that are neither free-to-play nor bordering
    /// free-to-play land are skipped.
    ///
    /// # Arguments
    /// * `members` - Whether this world loads members-only areas.
    /// * `lands` - The tile flags loaded by [`load_ground`](Self::load_ground).
    /// * `buf` - The decompressed mapsquare location data.
    /// * `cache` - The game cache for loc type lookups.
    /// * `zones` - The zone map to add static locs to.
    /// * `originx` - The mapsquare origin X in absolute tile coordinates.
    /// * `originz` - The mapsquare origin Z in absolute tile coordinates.
    ///
    /// # Side Effects
    /// * Applies loc collision via [`change_loc_collision`].
    /// * Adds static locs to the zone map.
    ///
    /// # Call Stack
    /// **Called by:** [`load`](Self::load)
    /// **Calls:** [`change_loc_collision`]
    fn load_locations(
        members: bool,
        lands: &[u8; MAPSQUARE],
        buf: &mut Packet,
        cache: &CacheStore,
        zones: &mut ZoneMap,
        originx: u16,
        originz: u16,
    ) {
        let mut id: i32 = -1;
        let mut id_offset = buf.gsmart1or2();
        while id_offset != 0 {
            id += id_offset;

            let mut coord: i32 = 0;
            let mut coord_offset = buf.gsmart1or2();

            while coord_offset != 0 {
                coord += coord_offset - 1;
                let coord = MapsquareCoordGrid::from(coord as u16);

                let info = buf.g1();
                coord_offset = buf.gsmart1or2();

                let absolute_x = originx + coord.x() as u16;
                let absolute_z = originz + coord.z() as u16;
                if !members
                    && !cache.is_free(absolute_x, absolute_z)
                    && !cache.borders_free(absolute_x, absolute_z)
                {
                    continue;
                }

                let bridged = if coord.y() == 1 {
                    (lands[coord.packed() as usize] & LINK_BELOW) == LINK_BELOW
                } else {
                    let coord = MapsquareCoordGrid::new(coord.x(), 1, coord.z());
                    (lands[coord.packed() as usize] & LINK_BELOW) == LINK_BELOW
                };

                let bridge = if bridged {
                    coord.y() as i8 - 1
                } else {
                    coord.y() as i8
                };
                if bridge < 0 {
                    continue;
                }

                let Some(loc_type) = cache.locs.get_by_id(id as u16) else {
                    continue;
                };

                let width = loc_type.width;
                let length = loc_type.length;

                let shape = LocShape::try_from_primitive(info >> 2).unwrap();
                let layer = LocShape::try_from_primitive(shape as u8).unwrap().layer();
                let angle = LocAngle::try_from_primitive(info & 0x3).unwrap();
                let coord = CoordGrid::new(
                    originx + coord.x() as u16,
                    bridge as u8,
                    originz + coord.z() as u16,
                );

                if loc_type.blockwalk {
                    change_loc_collision(
                        shape,
                        layer,
                        angle,
                        loc_type.blockrange,
                        length,
                        width,
                        loc_type.active,
                        coord,
                        true,
                    );
                }

                if let Some(active) = loc_type.active
                    && active
                {
                    let loc = Loc::new(
                        coord,
                        width,
                        length,
                        EntityLifeTime::Respawn,
                        id as u16,
                        shape,
                        angle,
                        layer,
                    );

                    zones
                        .zone_mut(coord.x(), coord.y(), coord.z())
                        .add_static_loc(loc);
                }
            }
            id_offset = buf.gsmart1or2();
        }
    }

    /// Decodes NPC spawn entries from the mapsquare buffer and creates
    /// `ActiveNpc` instances for each valid spawn.
    ///
    /// Members-only NPCs are skipped when `members` is false, as are all NPCs
    /// outside free-to-play areas on a free-to-play world.
    ///
    /// # Arguments
    /// * `members` - Whether members-only NPCs should be spawned.
    /// * `buf` - The decompressed mapsquare NPC data.
    /// * `cache` - The game cache for NPC type lookups.
    /// * `out` - The output vector to receive spawned NPCs.
    /// * `originx` - The mapsquare origin X in absolute tile coordinates.
    /// * `originz` - The mapsquare origin Z in absolute tile coordinates.
    ///
    /// # Call Stack
    /// **Called by:** [`load`](Self::load)
    /// **Calls:** [`ActiveNpc::new`]
    fn load_npcs(
        members: bool,
        buf: &mut Packet,
        cache: &CacheStore,
        out: &mut Vec<ActiveNpc>,
        originx: u16,
        originz: u16,
    ) {
        let mut id: i32 = -1;
        let mut id_offset = buf.gsmart1or2();
        while id_offset != 0 {
            id += id_offset;

            let mut coord: i32 = 0;
            let mut coord_offset = buf.gsmart1or2();

            while coord_offset != 0 {
                coord += coord_offset - 1;
                let coord = MapsquareCoordGrid::from(coord as u16);

                coord_offset = buf.gsmart1or2();

                let Some(npc) = cache.npcs.get_by_id(id as u16) else {
                    continue;
                };

                if npc.members && !members {
                    continue;
                }

                let coord = CoordGrid::new(
                    originx + coord.x() as u16,
                    coord.y(),
                    originz + coord.z() as u16,
                );

                if !members && !cache.is_free(coord.x(), coord.z()) {
                    continue;
                }

                let vars = VarSet::new(cache.varns.types.iter().map(|v| v.var_type));
                out.push(ActiveNpc::new(id as u16, 0, coord, npc.size, vars, cache));
            }
            id_offset = buf.gsmart1or2();
        }
    }

    /// Decodes ground object spawn entries from the mapsquare buffer and
    /// adds them as static objects in the zone map.
    ///
    /// Members-only objects are skipped when `members` is false, as are all
    /// objects outside free-to-play areas on a free-to-play world.
    ///
    /// # Arguments
    /// * `members` - Whether members-only objects should be spawned.
    /// * `buf` - The decompressed mapsquare object data.
    /// * `cache` - The game cache for object type lookups.
    /// * `zones` - The zone map to add static objs to.
    /// * `originx` - The mapsquare origin X in absolute tile coordinates.
    /// * `originz` - The mapsquare origin Z in absolute tile coordinates.
    ///
    /// # Side Effects
    /// * Adds static objs to the zone map.
    ///
    /// # Call Stack
    /// **Called by:** [`load`](Self::load)
    fn load_objs(
        members: bool,
        buf: &mut Packet,
        cache: &CacheStore,
        zones: &mut ZoneMap,
        originx: u16,
        originz: u16,
    ) {
        let mut id: i32 = -1;
        let mut id_offset = buf.gsmart1or2();
        while id_offset != 0 {
            id += id_offset;

            let mut coord: i32 = 0;
            let mut coord_offset = buf.gsmart1or2();

            while coord_offset != 0 {
                coord += coord_offset - 1;
                let coord = MapsquareCoordGrid::from(coord as u16);

                let count = buf.g2();
                coord_offset = buf.gsmart1or2();

                let Some(obj) = cache.objs.get_by_id(id as u16) else {
                    continue;
                };

                if obj.members && !members {
                    continue;
                }

                let coord = CoordGrid::new(
                    originx + coord.x() as u16,
                    coord.y(),
                    originz + coord.z() as u16,
                );

                if !members && !cache.is_free(coord.x(), coord.z()) {
                    continue;
                }

                let obj = Obj::new(coord, EntityLifeTime::Respawn, obj.id, count as u32);

                zones
                    .zone_mut(coord.x(), coord.y(), coord.z())
                    .add_static_obj(obj);
            }
            id_offset = buf.gsmart1or2();
        }
    }
}

/// Applies or removes collision for a location entity based on its type
/// definition in the cache.
///
/// Only modifies collision if the loc type has `blockwalk` set to `true`.
///
/// # Arguments
/// * `cache` - The game cache for loc type lookups.
/// * `loc` - The location entity whose collision is being changed.
/// * `coord` - The tile coordinate of the location.
/// * `add` - `true` to add collision, `false` to remove it.
///
/// # Call Stack
/// **Calls:** [`change_loc_collision`]
pub fn apply_loc_collision(cache: &CacheStore, loc: &Loc, coord: CoordGrid, add: bool) {
    if let Some(lt) = cache.locs.get_by_id(loc.id())
        && lt.blockwalk
    {
        change_loc_collision(
            loc.shape(),
            loc.layer(),
            loc.angle(),
            lt.blockrange,
            lt.length,
            lt.width,
            lt.active,
            coord,
            add,
        );
    }
}

/// Applies or removes collision for a location by its type ID and
/// shape/angle/layer parameters, looking up the type definition in the cache.
///
/// Only modifies collision if the loc type has `blockwalk` set to `true`.
///
/// # Arguments
/// * `cache` - The game cache for loc type lookups.
/// * `id` - The loc type ID.
/// * `shape` - The loc shape (e.g. wall, ground decor).
/// * `layer` - The loc layer derived from the shape.
/// * `angle` - The rotation angle of the loc.
/// * `coord` - The tile coordinate of the location.
/// * `add` - `true` to add collision, `false` to remove it.
///
/// # Call Stack
/// **Calls:** [`change_loc_collision`]
pub fn apply_collision_by_id(
    cache: &CacheStore,
    id: u16,
    shape: LocShape,
    layer: LocLayer,
    angle: LocAngle,
    coord: CoordGrid,
    add: bool,
) {
    if let Some(lt) = cache.locs.get_by_id(id)
        && lt.blockwalk
    {
        change_loc_collision(
            shape,
            layer,
            angle,
            lt.blockrange,
            lt.length,
            lt.width,
            lt.active,
            coord,
            add,
        );
    }
}

/// Adds or removes collision flags for a single location on the collision
/// map, dispatching to the appropriate `rsmod` function based on the
/// location's layer.
///
/// # Arguments
/// * `shape` - The loc shape.
/// * `layer` - The loc layer (Wall, WallDecor, Ground, GroundDecor).
/// * `angle` - The rotation angle of the loc.
/// * `blockrange` - Whether the loc blocks ranged projectiles.
/// * `length` - The loc's Z-axis tile size.
/// * `width` - The loc's X-axis tile size.
/// * `active` - If `Some(true)` for GroundDecor, a floor collision flag is
///   set.
/// * `coord` - The tile coordinate.
/// * `add` - `true` to add collision, `false` to remove it.
///
/// # Side Effects
/// * Calls `rsmod::change_wall`, `rsmod::change_loc`, or
///   `rsmod::change_floor` depending on the layer.
///
/// # Call Stack
/// **Called by:** [`GameMap::load_locations`], [`apply_loc_collision`],
/// [`apply_collision_by_id`]
#[allow(clippy::too_many_arguments)]
pub fn change_loc_collision(
    shape: LocShape,
    layer: LocLayer,
    angle: LocAngle,
    blockrange: bool,
    length: u8,
    width: u8,
    active: Option<bool>,
    coord: CoordGrid,
    add: bool,
) {
    let x = coord.x();
    let y = coord.y();
    let z = coord.z();
    match layer {
        LocLayer::Wall => {
            rsmod::change_wall(x, z, y, angle as u8, shape as i8, blockrange, false, add)
        }
        LocLayer::WallDecor => {}
        LocLayer::Ground => match angle {
            LocAngle::North | LocAngle::South => {
                rsmod::change_loc(x, z, y, length, width, blockrange, false, add)
            }
            LocAngle::West | LocAngle::East => {
                rsmod::change_loc(x, z, y, width, length, blockrange, false, add)
            }
        },
        LocLayer::GroundDecor => {
            if active == Some(true) {
                rsmod::change_floor(x, z, y, add);
            }
        }
    }
}
