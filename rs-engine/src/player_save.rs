use rs_entity::player::{
    ChatSettingsPrivate, ChatSettingsPublic, ChatSettingsTradeDuel, Player, StaffModLevel,
};
use rs_grid::CoordGrid;
use rs_inv::{Inventory, StackMode};
use rs_io::{Packet, crc};
use rs_pack::cache::inv::InvScope;
use rs_pack::cache::varp::VarPlayerScope;
use rs_pack::cache::{CacheStore, VarValue};
use rs_pack::types::PlayerStat;
use rs_stat::{get_exp_by_level, get_level_by_exp};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::error;

/// Magic number at the start of every binary save file for format validation.
const SAV_MAGIC: u16 = 0x2004;

/// Current save file format version. Incremented when the binary layout changes.
const SAV_VERSION: u16 = 7;

/// Total number of player stats (skills).
const STAT_COUNT: usize = 21;

// ---- PlayerProfile: shared representation for DB and binary ----

/// Represents a single inventory's contents within a player profile,
/// storing the inventory type ID and a list of (slot, obj_id, count) tuples.
pub struct PlayerProfileInv {
    pub inv_type: u16,
    pub items: Vec<(u16, u16, u32)>, // (slot, obj_id, count)
}

/// A serialisable snapshot of a player's persistent state.
///
/// Used as the intermediate representation between the live [`Player`] entity
/// and both the database and binary save file formats.
pub struct PlayerProfile {
    pub x: u16,
    pub z: u16,
    pub y: u8,
    pub body: [i32; 7],
    pub colors: [u8; 5],
    pub gender: u8,
    pub runenergy: u16,
    pub playtime: i32,
    pub stats: [i32; 21],
    pub levels: [u8; 21],
    pub varps: Vec<(u16, i32)>,
    pub invs: Vec<PlayerProfileInv>,
    pub afk_zones: [u32; 2],
    pub last_afk_zone: u16,
    pub public_chat: u8,
    pub private_chat: u8,
    pub trade_chat: u8,
    pub last_date: i64,
    pub staff_mod_level: u8,
}

/// Extracts a [`PlayerProfile`] from a live [`Player`] entity.
///
/// Only persistent-scope varps and inventories are included (temp-scope
/// data is discarded).
///
/// # Arguments
/// * `player` - The live player entity to extract from.
/// * `cache` - The game cache, used to determine varp and inventory scopes.
///
/// # Returns
/// A [`PlayerProfile`] containing all persistent player state.
pub fn extract_profile(player: &Player, cache: &CacheStore) -> PlayerProfile {
    let mut varps = Vec::new();
    for i in 0..player.vars.len() {
        let id = i as u16;
        let scope = cache
            .varps
            .get_by_id(id)
            .map(|v| v.scope)
            .unwrap_or(VarPlayerScope::Temp);
        if scope == VarPlayerScope::Perm {
            let value = player.vars.get(id).as_int();
            if value != 0 {
                varps.push((id, value));
            }
        }
    }

    let mut invs = Vec::new();
    for (&type_id, inventory) in &player.invs {
        let scope = cache
            .invs
            .get_by_id(type_id)
            .map(|v| v.scope)
            .unwrap_or(InvScope::Temp);
        if scope != InvScope::Perm {
            continue;
        }
        let mut items = Vec::new();
        for (slot, item) in inventory.slots.iter().enumerate() {
            if let Some(item) = item {
                items.push((slot as u16, item.obj, item.num));
            }
        }
        if !items.is_empty() {
            invs.push(PlayerProfileInv {
                inv_type: type_id,
                items,
            });
        }
    }

    PlayerProfile {
        x: player.pathing.coord.x(),
        z: player.pathing.coord.z(),
        y: player.pathing.coord.y(),
        body: player.body,
        colors: player.colours,
        gender: player.gender,
        runenergy: player.runenergy,
        playtime: player.playtime,
        stats: player.stats.xp,
        levels: player.stats.levels.map(|l| l as u8),
        varps,
        invs,
        afk_zones: player.afk_zones,
        last_afk_zone: player.last_afk_zone,
        public_chat: player.public as u8,
        private_chat: player.private as u8,
        trade_chat: player.trade as u8,
        last_date: player.last_date,
        staff_mod_level: player.staff_mod_level as u8,
    }
}

/// Applies a loaded [`PlayerProfile`] onto a live [`Player`] entity,
/// restoring all persistent state.
///
/// Sets coordinates, appearance, stats, levels, varps, inventories,
/// chat settings, and recalculates base levels and combat level.
///
/// # Arguments
/// * `profile` - The profile data to apply.
/// * `player` - The live player entity to modify.
/// * `cache` - The game cache, used for varp types and inventory sizes.
///
/// # Side Effects
/// * Overwrites player coordinates, stats, levels, appearance, varps,
///   inventories, and chat settings.
/// * Recalculates `base_levels` from stats and updates `combat_level`.
///
/// # Call Stack
/// **Calls:** [`get_level_by_exp`]
pub fn apply_profile(profile: &PlayerProfile, player: &mut Player, cache: &CacheStore) {
    player.pathing.coord = CoordGrid::new(profile.x, profile.y, profile.z);
    player.body = profile.body;
    player.colours = profile.colors;
    player.gender = profile.gender;
    player.runenergy = profile.runenergy;
    player.playtime = profile.playtime;
    player.stats.xp = profile.stats;
    player.stats.levels = profile.levels.map(|l| l as u16);
    for i in 0..STAT_COUNT {
        player.stats.base_levels[i] = get_level_by_exp(profile.stats[i]) as u16;
    }
    player.combat_level = player.get_combat_level();
    player.afk_zones = profile.afk_zones;
    player.last_afk_zone = profile.last_afk_zone;
    player.last_date = profile.last_date;
    player.last_login_date = profile.last_date;
    player.staff_mod_level = StaffModLevel::from_u8(profile.staff_mod_level);

    player.public = ChatSettingsPublic::from_u8(profile.public_chat);
    player.private = ChatSettingsPrivate::from_u8(profile.private_chat);
    player.trade = ChatSettingsTradeDuel::from_u8(profile.trade_chat);

    for &(id, value) in &profile.varps {
        if (id as usize) < player.vars.len()
            && let Some(varp_type) = cache.varps.get_by_id(id)
        {
            player
                .vars
                .set(id, VarValue::from_int(varp_type.var_type, value));
        }
    }

    for inv_profile in &profile.invs {
        let inv_type = cache.invs.get_by_id(inv_profile.inv_type);
        if inv_type.map(|t| t.scope).unwrap_or(InvScope::Temp) != InvScope::Perm {
            continue;
        }
        let capacity = inv_type.map(|t| t.size as usize).unwrap_or(28);
        let stack_mode = if inv_type.is_some_and(|t| t.stackall) {
            StackMode::Always
        } else {
            StackMode::Normal
        };
        let mut inv = Inventory::with_stack_mode(capacity, stack_mode);
        for &(slot, obj_id, count) in &inv_profile.items {
            if (slot as usize) < inv.capacity && cache.objs.get_by_id(obj_id).is_some() {
                inv.set(slot, obj_id, count);
            }
        }
        player.invs.insert(inv_profile.inv_type, inv);
    }
}

// ---- Binary serialization (local file fallback, TS-compatible) ----

/// Serializes a [`PlayerProfile`] into the binary `.sav` file format.
///
/// Writes the magic number, version, coordinates, appearance, stats,
/// varps, inventories, AFK zones, chat settings, last login date, and a
/// CRC32 checksum.
///
/// # Arguments
/// * `profile` - The player profile to serialise.
/// * `cache` - The game cache, used for varp count and inventory sizes.
///
/// # Returns
/// A `Vec<u8>` containing the complete binary save data.
pub fn save_binary(profile: &PlayerProfile, cache: &CacheStore) -> Vec<u8> {
    let mut sav = Packet::new(5000);

    sav.p2(SAV_MAGIC);
    sav.p2(SAV_VERSION);

    sav.p2(profile.x);
    sav.p2(profile.z);
    sav.p1(profile.y);

    for i in 0..7 {
        sav.p1(profile.body[i] as u8);
    }
    for i in 0..5 {
        sav.p1(profile.colors[i]);
    }
    sav.p1(profile.gender);

    sav.p2(profile.runenergy);
    sav.p4(profile.playtime);

    for i in 0..STAT_COUNT {
        sav.p4(profile.stats[i]);
        sav.p1(profile.levels[i]);
    }

    let varp_count = cache.varps.count() as u16;
    sav.p2(varp_count);
    for i in 0..varp_count {
        let scope = cache
            .varps
            .get_by_id(i)
            .map(|v| v.scope)
            .unwrap_or(VarPlayerScope::Temp);
        if scope == VarPlayerScope::Perm {
            let value = profile
                .varps
                .iter()
                .find(|(id, _)| *id == i)
                .map(|(_, v)| *v)
                .unwrap_or(0);
            sav.p4(value);
        } else {
            sav.p4(0);
        }
    }

    let inv_count_pos = sav.pos;
    sav.p1(0);
    let mut inv_count: u8 = 0;

    for inv_profile in &profile.invs {
        let inv_type = cache.invs.get_by_id(inv_profile.inv_type);
        let capacity = inv_type.map(|t| t.size as usize).unwrap_or(28);

        sav.p2(inv_profile.inv_type);
        sav.p2(capacity as u16);
        for slot in 0..capacity {
            if let Some(&(_, obj_id, count)) = inv_profile
                .items
                .iter()
                .find(|(s, _, _)| *s as usize == slot)
            {
                sav.p2(obj_id + 1);
                if count >= 255 {
                    sav.p1(255);
                    sav.p4(count as i32);
                } else {
                    sav.p1(count as u8);
                }
            } else {
                sav.p2(0);
            }
        }
        inv_count += 1;
    }
    sav.data[inv_count_pos] = inv_count;

    sav.p1(profile.afk_zones.len() as u8);
    for &zone in &profile.afk_zones {
        sav.p4(zone as i32);
    }
    sav.p2(profile.last_afk_zone);

    let packed_chat = (profile.public_chat << 4) | (profile.private_chat << 2) | profile.trade_chat;
    sav.p1(packed_chat);

    sav.p8(profile.last_date);
    sav.p1(profile.staff_mod_level);

    let checksum = crc::getcrc(&sav.data, 0, sav.pos);
    sav.p4(checksum);

    sav.data.truncate(sav.pos);
    sav.data
}

/// DeSerializes a [`PlayerProfile`] from raw binary `.sav` data.
///
/// Validates the magic number, version, and CRC32 checksum before parsing.
/// Supports forward-compatible reading of older save versions (v2..=v6).
///
/// # Arguments
/// * `data` - The raw binary save data.
///
/// # Returns
/// `Ok(profile)` on success, or an `Err` with a static error message if
/// the data is too short, has an invalid magic, unsupported version, or
/// incorrect checksum.
/// # ★★ THE BOUNDS CHECK `rs_io::Packet`'S OWN GETTERS DON'T HAVE.
///
/// `Packet::g1` through `g8s` (vendored `rs-io-0.3.1`, `src/packet.rs`) each do
/// a raw `unsafe { *self.data.as_ptr().add(self.pos) }` /
/// `read_unaligned` with NO check against `self.data.len()`. Reading past a
/// short buffer through them is undefined behaviour -- a segfault or worse,
/// not a catchable panic -- and `load_binary`'s only two gates before the
/// first `g1`/`g2` call (a magic number and a CRC32 computed over whatever
/// bytes are actually present) are both trivially satisfiable by a blob
/// whose length disagrees with what its own `varp_count`, `inv_count`, or an
/// inventory's `size` field claims: an attacker who controls every byte can
/// always pick a CRC that matches.
///
/// This matters here specifically because `host_load_profile` (Phase 1's C
/// ABI, `rs-host/src/lib.rs`) is the FIRST call site where `load_binary`
/// receives bytes from outside an engine-authored `data/players/*.sav` file.
/// `need(n)` before a read (or a run of reads) turns "read past the end" into
/// an `Err` the caller already handles, instead of a dead process.
///
/// Not `sav.remaining()` (which exists on `Packet`): that casts `len - pos`
/// to `i32` and this crate targets buffers that only need to stay under
/// `usize`, so this stays in `usize` and uses `checked_add` to also catch the
/// (should-be-impossible, but let's not assume) case of `pos` having already
/// overrun `len`.
fn need(sav: &Packet, n: usize) -> Result<(), &'static str> {
    match sav.pos.checked_add(n) {
        Some(end) if end <= sav.data.len() => Ok(()),
        _ => Err("Save data truncated"),
    }
}

pub fn load_binary(data: &[u8]) -> Result<PlayerProfile, &'static str> {
    if data.len() < 4 {
        return Err("Save data too short");
    }

    let mut sav = Packet::from(data.to_vec());

    let magic = sav.g2();
    if magic != SAV_MAGIC {
        return Err("Invalid save magic");
    }

    let version = sav.g2();
    if version > SAV_VERSION {
        return Err("Unsupported save version");
    }

    let crc_pos = sav.data.len() - 4;
    let stored_crc = i32::from_be_bytes([
        sav.data[crc_pos],
        sav.data[crc_pos + 1],
        sav.data[crc_pos + 2],
        sav.data[crc_pos + 3],
    ]);
    let computed_crc = crc::getcrc(&sav.data, 0, crc_pos);
    if stored_crc != computed_crc {
        return Err("Incorrect save checksum");
    }

    // ★★ Everything from here on reads through `Packet`'s unchecked getters,
    // so every read (or contiguous run of fixed-size reads) is preceded by a
    // `need` call. See `need`'s doc comment.
    need(&sav, 2 + 2 + 1 + 7 + 5 + 1 + 2)?; // x, z, y, body[7], colors[5], gender, runenergy
    let x = sav.g2();
    let z = sav.g2();
    let y = sav.g1();

    let mut body = [0i32; 7];
    for b in &mut body {
        let v = sav.g1() as i32;
        *b = if v == 255 { -1 } else { v };
    }
    let mut colors = [0u8; 5];
    for c in &mut colors {
        *c = sav.g1();
    }
    let gender = sav.g1();

    let runenergy = sav.g2();
    need(&sav, if version >= 2 { 4 } else { 2 })?;
    let playtime = if version >= 2 {
        sav.g4s()
    } else {
        sav.g2() as i32
    };

    need(&sav, STAT_COUNT * 5)?; // per stat: a g4s xp (4) + a g1 level (1)
    let mut stats = [0i32; 21];
    let mut levels = [1u8; 21];
    for i in 0..STAT_COUNT {
        stats[i] = sav.g4s();
        levels[i] = sav.g1();
    }

    need(&sav, 2)?;
    let varp_count = sav.g2() as usize;
    // ★ THE RUNAWAY LOOP, #1: `varp_count` is a caller-controlled u16 that
    // directly drives this loop's length -- two bytes can claim up to 65535
    // iterations of a 4-byte read. Checking the WHOLE claimed span up front,
    // rather than per-iteration, is what turns "reads 256KB past an 8-byte
    // buffer" into one `Err` before the loop starts.
    need(&sav, varp_count * 4)?;
    let mut varps = Vec::new();
    for i in 0..varp_count {
        let value = sav.g4s();
        if value != 0 {
            varps.push((i as u16, value));
        }
    }

    need(&sav, 1)?;
    let inv_count = sav.g1() as usize;
    let mut invs = Vec::new();
    for _ in 0..inv_count {
        need(&sav, 2)?;
        let type_id = sav.g2();
        if version < 5 {
            return Err("Save version too old for inv capacity");
        }
        need(&sav, 2)?;
        let size = sav.g2() as usize;

        // ★ THE RUNAWAY LOOP, #2: `size` is exactly as caller-controlled as
        // `varp_count`, but unlike it, each slot's own width (2, 3, or 7
        // bytes) depends on bytes not yet read -- so unlike the varp loop
        // above, this cannot be checked as one span up front. Each read is
        // gated individually instead.
        let mut items = Vec::new();
        for slot in 0..size {
            need(&sav, 2)?;
            let id_raw = sav.g2();
            if id_raw == 0 {
                continue;
            }
            let id = id_raw - 1;
            need(&sav, 1)?;
            let count_byte = sav.g1();
            let count = if count_byte == 255 {
                need(&sav, 4)?;
                sav.g4s() as u32
            } else {
                count_byte as u32
            };
            items.push((slot as u16, id, count));
        }

        if !items.is_empty() {
            invs.push(PlayerProfileInv {
                inv_type: type_id,
                items,
            });
        }
    }

    let mut afk_zones = [0u32; 2];
    let mut last_afk_zone: u16 = 0;
    if version >= 3 {
        need(&sav, 1)?;
        let afk_count = sav.g1() as usize;
        // ★ A THIRD caller-controlled count (`afk_count`, a u8): bounded to
        // 255 iterations rather than 65535, but the same class of bug.
        need(&sav, afk_count * 4)?;
        for z in afk_zones.iter_mut().take(afk_count.min(2)) {
            *z = sav.g4s() as u32;
        }
        for _ in 2..afk_count {
            sav.g4s();
        }
        need(&sav, 2)?;
        last_afk_zone = sav.g2();
    }

    let (public_chat, private_chat, trade_chat) = if version >= 4 {
        need(&sav, 1)?;
        let packed = sav.g1();
        ((packed >> 4) & 0b11, (packed >> 2) & 0b11, packed & 0b11)
    } else {
        (0, 0, 0)
    };

    let last_date = if version >= 6 {
        need(&sav, 8)?;
        sav.g8s()
    } else {
        0
    };
    // Saves older than v7 predate staff-level persistence; default to Normal (0).
    let staff_mod_level = if version >= 7 {
        need(&sav, 1)?;
        sav.g1()
    } else {
        0
    };

    Ok(PlayerProfile {
        x,
        z,
        y,
        body,
        colors,
        gender,
        runenergy,
        playtime,
        stats,
        levels,
        varps,
        invs,
        afk_zones,
        last_afk_zone,
        public_chat,
        private_chat,
        trade_chat,
        last_date,
        staff_mod_level,
    })
}

// ---- New player defaults ----

/// Applies default stats and levels to a newly created player.
///
/// Sets all stats to 0 and all levels to 1, except Hitpoints which is
/// initialized to level 10 with the corresponding experience. Recalculates
/// combat level afterward.
///
/// # Arguments
/// * `player` - The new player entity to initialise.
///
/// # Side Effects
/// * Zeroes all stats and sets all levels to 1.
/// * Sets Hitpoints stat and level to 10.
/// * Recalculates `combat_level`.
///
/// # Call Stack
/// **Calls:** [`get_exp_by_level`]
pub fn apply_new_player_defaults(player: &mut Player) {
    for i in 0..STAT_COUNT {
        player.stats.xp[i] = 0;
        player.stats.levels[i] = 1;
        player.stats.base_levels[i] = 1;
    }
    player.stats.xp[PlayerStat::Hitpoints as usize] = get_exp_by_level(10);
    player.stats.levels[PlayerStat::Hitpoints as usize] = 10;
    player.stats.base_levels[PlayerStat::Hitpoints as usize] = 10;
    player.combat_level = player.get_combat_level();
}

// ---- File I/O ----

/// Builds the on-disk save path `data/players/{username}.sav` for a player.
fn save_path(username: &str) -> PathBuf {
    Path::new("data")
        .join("players")
        .join(format!("{}.sav", username))
}

/// Writes binary save data to a local file at `data/players/{username}.sav`.
///
/// Creates the `data/players/` directory if it does not exist.
///
/// # Arguments
/// * `username` - The player's username (used as the filename stem).
/// * `data` - The binary save data to write.
///
/// # Side Effects
/// * Creates or overwrites the `.sav` file on disk.
/// * Logs an error if the directory or file cannot be created.
pub fn save_to_file(username: &str, data: &[u8]) {
    let dir = Path::new("data").join("players");
    if let Err(e) = fs::create_dir_all(&dir) {
        error!("Failed to create save directory: {}", e);
        return;
    }
    let path = save_path(username);
    match fs::File::create(&path).and_then(|mut f| f.write_all(data)) {
        Ok(()) => {}
        Err(e) => error!("Failed to write save file for '{}': {}", username, e),
    }
}

/// Reads binary save data from the local file `data/players/{username}.sav`.
///
/// # Arguments
/// * `username` - The player's username (used as the filename stem).
///
/// # Returns
/// `Some(data)` if the file exists and is readable, `None` otherwise.
pub fn load_from_file(username: &str) -> Option<Vec<u8>> {
    let path = save_path(username);
    fs::read(&path).ok()
}

/// Deletes the local save file `data/players/{username}.sav` if it exists.
///
/// Silently ignores errors (e.g. file does not exist).
///
/// # Arguments
/// * `username` - The player's username (used as the filename stem).
pub fn delete_save_file(username: &str) {
    let path = save_path(username);
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_player_defaults_hitpoints() {
        let uid = rs_vm::PlayerUid::new("test".into(), 1);
        let vars = rs_var::VarSet::new(std::iter::empty());
        let mut player = Player::new(uid, CoordGrid::new(3222, 0, 3222), vars, false);
        apply_new_player_defaults(&mut player);
        assert_eq!(player.stats.levels[PlayerStat::Hitpoints as usize], 10);
        assert!(player.stats.xp[PlayerStat::Hitpoints as usize] > 0);
        for i in 0..STAT_COUNT {
            if i != PlayerStat::Hitpoints as usize {
                assert_eq!(player.stats.xp[i], 0);
                assert_eq!(player.stats.levels[i], 1);
            }
        }
    }

    /// Writes a well-formed save prefix through `magic` .. the stats/levels
    /// loop (everything up to, but not including, `varp_count`), using
    /// `SAV_VERSION` and all-zero/level-1 values. Shared by the two
    /// truncation regression tests below so each one only has to hand-craft
    /// the ONE section it means to break.
    fn write_valid_prefix(sav: &mut Packet) {
        sav.p2(SAV_MAGIC);
        sav.p2(SAV_VERSION);
        sav.p2(0); // x
        sav.p2(0); // z
        sav.p1(0); // y
        for _ in 0..7 {
            sav.p1(0); // body
        }
        for _ in 0..5 {
            sav.p1(0); // colors
        }
        sav.p1(0); // gender
        sav.p2(0); // runenergy
        sav.p4(0); // playtime (version >= 2)
        for _ in 0..STAT_COUNT {
            sav.p4(0); // xp
            sav.p1(1); // level
        }
    }

    /// Appends the CRC32 trailer `load_binary` checks, computed over exactly
    /// the bytes written so far -- the same recipe `save_binary` uses. This is
    /// what makes a crafted-and-truncated blob pass the CRC gate: the two
    /// tests below are not "a save file with a byte chopped off" (which would
    /// fail CRC before reaching any vulnerable read, proving nothing) but a
    /// deliberately shorter blob whose CRC is honestly computed over its own,
    /// shorter content.
    fn sign_crc(sav: &mut Packet) {
        let checksum = crc::getcrc(&sav.data, 0, sav.pos);
        sav.p4(checksum);
        sav.data.truncate(sav.pos);
    }

    /// ★★ Regression for the Critical finding on Task 1's `host_load_profile`
    /// round: `Packet::g1`..`g4s` (vendored `rs-io`) do raw, unchecked pointer
    /// reads with no bounds check against `data.len()`. Before `need` existed,
    /// `varp_count` -- a caller-controlled u16 read straight off the wire --
    /// drove a loop of `varp_count` unchecked 4-byte reads with nothing
    /// stopping it from walking off the end of a short buffer: undefined
    /// behaviour, not a catchable error. This blob claims `varp_count = 100`
    /// (400 bytes) but supplies only 8 real bytes of varp data before the CRC
    /// trailer, and its CRC is honestly computed over that shorter body -- so
    /// it clears both of `load_binary`'s existing gates (magic, CRC) and would
    /// have reached the unchecked reads. It must come back `Err`, not crash.
    #[test]
    fn load_binary_rejects_a_blob_truncated_inside_the_varp_section() {
        let mut sav = Packet::new(256);
        write_valid_prefix(&mut sav);
        sav.p2(100); // varp_count claims 100 entries (400 bytes)...
        for _ in 0..2 {
            sav.p4(0); // ...but only 8 bytes of them actually follow.
        }
        sign_crc(&mut sav);

        match load_binary(&sav.data) {
            Err(_) => {}
            Ok(_) => panic!(
                "a blob claiming 100 varp entries with only 2 present should not parse \
                 successfully -- either it read garbage or (worse) it read past the buffer"
            ),
        }
    }

    /// ★★ Same finding, the other runaway loop: an inventory's `size` field is
    /// exactly as caller-controlled as `varp_count`, and drives the per-slot
    /// item loop. This blob claims one inventory of `size = 50` slots but
    /// supplies only 2 slots' worth of item bytes before the CRC trailer.
    #[test]
    fn load_binary_rejects_a_blob_truncated_inside_an_inventory() {
        let mut sav = Packet::new(256);
        write_valid_prefix(&mut sav);
        sav.p2(0); // varp_count: none, so the varp section is legitimately empty
        sav.p1(1); // inv_count: one inventory
        sav.p2(0); // inv_type
        sav.p2(50); // size claims 50 slots (each at least 2 bytes)...
        for _ in 0..2 {
            sav.p2(1); // id_raw = 1 (non-zero, so a count byte follows each)
            sav.p1(1); // count_byte
        }
        // ...but only 2 slots' worth of item bytes actually follow.
        sign_crc(&mut sav);

        match load_binary(&sav.data) {
            Err(_) => {}
            Ok(_) => panic!(
                "a blob claiming a 50-slot inventory with only 2 slots' bytes present \
                 should not parse successfully -- either it read garbage or (worse) it \
                 read past the buffer"
            ),
        }
    }
}
