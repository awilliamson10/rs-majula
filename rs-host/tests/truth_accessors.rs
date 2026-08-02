//! The LABEL CHANNEL: engine truth about npcs, and the attack cooldown.
//!
//! ★★ WHAT THIS IS FOR, because it decides what counts as a bug here. The
//! milestone asks whether a model trained only on the CLIENT's information set
//! builds an internal representation of state the client was never sent. These
//! accessors produce the ground truth that question is scored against. So a
//! silently-wrong reading here does not fail loudly — it produces a probe result
//! that looks like a discovery and measures nothing.
//!
//! ★ ONE ENGINE PER PROCESS: this binary calls the constructor exactly once, and
//! therefore holds exactly ONE `#[test]` fn. `rs-pathfinder` keeps
//! `COLLISION_FLAGS` process-global and `host_new_at` asserts on a second call,
//! so splitting the assertions below into separate test fns would abort the
//! binary rather than run them.

use std::ffi::CString;

/// ★ A LITERAL, not `rs_host::HOST_FIELD_UNKNOWN`. A test that imported the
/// constant would keep passing if the constant itself changed to something a
/// real field can hold — the same reason `tests/varp_unknown.rs` duplicates
/// `HOST_VARP_UNKNOWN`.
const HOST_FIELD_UNKNOWN: i64 = i64::MIN;

/// Lumbridge, the mainland respawn. Chosen because a mainland login grants the
/// sidebar tabs and leaves `%tutorial` at 0 (`host_new_at`'s doc comment), and
/// because the castle grounds are densely populated — a spawn with no npcs in
/// range would make every assertion below vacuous.
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

/// Field ids, mirroring `host_npc_field`'s doc comment.
const F_NID: u32 = 0;
const F_HP: u32 = 1;
const F_MAX_HP: u32 = 2;
const F_RESPAWN_AT: u32 = 3;
const F_TARGET_PLAYER: u32 = 4;
const F_HUNT_MODE: u32 = 5;
const F_TILE_X: u32 = 6;
const F_TILE_Z: u32 = 7;
const F_TYPE_ID: u32 = 8;
const F_ACTIVE: u32 = 9;

#[test]
fn npc_truth_reads_real_values_and_never_panics() {
    let (x, level, z) = LUMBRIDGE;
    let h = rs_host::host_new_at(4242, x, level, z);
    assert!(!h.is_null());

    for _ in 0..20 {
        rs_host::host_step(h);
    }

    // -- the slot list ---------------------------------------------------------

    let n = rs_host::host_npc_count(h);
    assert!(
        n > 0,
        "a mainland spawn should have npcs in range — otherwise every label frame \
         is empty and the whole channel is untested"
    );

    // -- ★★ THE HITPOINTS INDEX, proven against an INDEPENDENT path -----------
    //
    // `Stats<6>` is a bare `[u16; 6]`; nothing in it says which slot is
    // hitpoints, and reading the wrong one reports (say) strength as HP with no
    // error anywhere. NPC hp is this milestone's POSITIVE CONTROL for the
    // hidden-state probe, so a wrong index means the control passes or fails for
    // reasons unrelated to anything being measured.
    //
    // The implementation uses `rs_pack::types::NpcStat::Hitpoints`, the engine's
    // own enum. This checks it a second, genuinely independent way: against the
    // npc TYPE CONFIG packed out of `content/274`. `ActiveNpc::new` seeds
    // `base_levels[Hitpoints]` from `npc_type.hitpoints`, and nothing on a live,
    // undamaged npc changes a base level — so config and engine must agree. If
    // the accessor read index 2 (strength) or 4 (ranged) instead, this fails for
    // every npc whose hitpoints differ from that stat, which at Lumbridge is
    // most of them.
    let cache = rl_env::cache();
    let mut checked = 0usize;
    let mut distinct_max_hp = std::collections::BTreeSet::new();
    for slot in 0..n {
        let type_id = rs_host::host_npc_field(h, slot, F_TYPE_ID);
        assert_ne!(type_id, HOST_FIELD_UNKNOWN, "slot {slot} has no type id");
        let Some(npc_type) = cache.npcs.get_by_id(type_id as u16) else {
            continue;
        };
        let max = rs_host::host_npc_field(h, slot, F_MAX_HP);
        assert_eq!(
            max,
            npc_type.hitpoints as i64,
            "slot {slot} (npc type {type_id}, {:?}): the engine's max hp disagrees with \
             the packed config's `hitpoints`. This is the hitpoints-index check — if it \
             fails, `host_npc_field` is reading the wrong slot of `Stats<6>` and every \
             hp label in the milestone is a different stat.",
            npc_type.debugname()
        );
        distinct_max_hp.insert(max);
        checked += 1;
    }
    assert!(
        checked > 0,
        "no npc in range resolved to a cache type — the hitpoints-index check above \
         ran zero times and proved nothing"
    );
    // ★ NON-VACUITY for the loop: an accessor that returned a CONSTANT would
    // satisfy the equality above only if the config were also constant. Lumbridge
    // holds several npc types (men, women, guards, a cook, Hans...) with
    // different hitpoints, so more than one value must appear.
    assert!(
        distinct_max_hp.len() > 1,
        "every npc in range reports the same max hp ({distinct_max_hp:?}) — the \
         accessor may be returning a constant rather than reading each npc"
    );

    // -- current hp is a real, paired reading ----------------------------------
    //
    // Without the pairing against max, an accessor returning any positive
    // constant passes a mere "is not the sentinel" check.
    for slot in 0..n {
        let cur = rs_host::host_npc_field(h, slot, F_HP);
        let max = rs_host::host_npc_field(h, slot, F_MAX_HP);
        assert!(
            cur > 0 && cur <= max,
            "slot {slot}: hp {cur} out of range against max {max}"
        );
    }

    // -- the rest of the row ---------------------------------------------------

    for slot in 0..n {
        let nid = rs_host::host_npc_field(h, slot, F_NID);
        assert!(nid >= 0, "slot {slot}: nid {nid}");

        // ★ -1 means `None`, and that is exactly why the SENTINEL is not -1:
        // `respawn_at`, `target_player` and `hunt_mode` all legitimately report
        // it, so a -1 sentinel would make "this npc has no target" and "there is
        // no such npc" indistinguishable — the ambiguity that corrupts a label.
        for field in [F_RESPAWN_AT, F_TARGET_PLAYER, F_HUNT_MODE] {
            let v = rs_host::host_npc_field(h, slot, field);
            assert!(v >= -1, "slot {slot} field {field}: {v} is below -1");
            assert_ne!(v, HOST_FIELD_UNKNOWN, "slot {slot} field {field}");
        }

        // The npcs are where the PLAYER is, not at the world origin — an
        // enumeration that ignored the player's coordinate would put them
        // anywhere.
        let nx = rs_host::host_npc_field(h, slot, F_TILE_X);
        let nz = rs_host::host_npc_field(h, slot, F_TILE_Z);
        let px = rs_host::host_player_x(h) as i64;
        let pz = rs_host::host_player_z(h) as i64;
        assert!(
            (nx - px).abs() <= 15 && (nz - pz).abs() <= 15,
            "slot {slot} at ({nx}, {nz}) is outside the label radius of ({px}, {pz})"
        );

        let active = rs_host::host_npc_field(h, slot, F_ACTIVE);
        assert!(active == 0 || active == 1, "slot {slot}: active {active}");
    }

    // ★ The slot list is SORTED BY NID, which is what makes slot i mean the same
    // npc on two consecutive ticks. Unsorted, a label file could only be joined
    // across ticks by carrying nid through every consumer.
    let nids: Vec<i64> = (0..n).map(|s| rs_host::host_npc_field(h, s, F_NID)).collect();
    let mut sorted = nids.clone();
    sorted.sort_unstable();
    assert_eq!(nids, sorted, "the slot list must be sorted by nid");

    // -- ★★ MUST NOT PANIC ----------------------------------------------------
    //
    // Every panic in an `extern "C"` fn ABORTS the process: the runtime cannot
    // unwind across a C frame, so there is no JS-visible error, just a dead host.
    // That is not theoretical — `host_varp` used to kill the fused host outright
    // on an unknown varp name. Before the bounds checks in `host_npc_field`
    // these two lines kill this test binary rather than fail it.
    assert_eq!(rs_host::host_npc_field(h, 99_999, F_HP), HOST_FIELD_UNKNOWN);
    assert_eq!(rs_host::host_npc_field(h, 0, 99), HOST_FIELD_UNKNOWN);
    assert_eq!(
        rs_host::host_npc_field(h, u32::MAX, u32::MAX),
        HOST_FIELD_UNKNOWN
    );

    // -- the attack cooldown ---------------------------------------------------
    //
    // ★ NO NEW ACCESSOR. `action_delay` is a plain player varp (id 58 in
    // `content/274/pack/varp.pack`), so `host_varp` already reaches it. What IS
    // new is `host_clock`, and it is not optional: the varp holds an ABSOLUTE
    // tick, not a countdown — content compares it against `map_clock`
    // (`quest_legends/scripts/jungle_tree.rs2:61`) — so the cooldown is
    // `max(0, action_delay - clock)` and the raw varp alone is a monotonically
    // rising number that would train a model on nonsense.
    let name = CString::new("action_delay").unwrap();
    let delay = rs_host::host_varp(h, name.as_ptr());
    assert_ne!(
        delay,
        i32::MIN,
        "`action_delay` must resolve through host_varp — if it does not, the cache \
         has no such varp and the cooldown label has no source"
    );

    let clock = rs_host::host_clock(h);
    assert!(clock > 0, "the engine clock must have advanced past 0, got {clock}");
    for _ in 0..3 {
        rs_host::host_step(h);
    }
    assert_eq!(
        rs_host::host_clock(h),
        clock + 3,
        "host_clock must track the engine's own tick counter"
    );

    // ★★ AND `action_delay` MUST NOT BE TRANSMITTED. This is the premise of the
    // whole label: a varp with `transmit=yes` is pushed to the client as a
    // `VarpSmall`/`VarpLarge` and lands in `client.var`, which would put the
    // attack cooldown INSIDE the client's information set. The probe would then
    // be reading out a column it was handed while reporting an inference.
    // `[action_delay]` in `content/274/scripts/_unpack/225/all.varp` declares no
    // `transmit`, and `VarPlayerType::new` defaults it to false — pinned here so
    // a content change fails loudly instead of quietly voiding the experiment.
    let varp = cache
        .varps
        .get_by_debugname("action_delay")
        .expect("action_delay must exist in the packed varp table");
    assert!(
        !varp.transmit,
        "`action_delay` is now TRANSMITTED to the client — it is no longer hidden \
         state and must not be used as a probe target"
    );

    // -- the count is a live reading, not a boot-time snapshot -----------------
    //
    // ★ The player has not moved, so the exact count may legitimately change by
    // a few as npcs wander in and out of range; what must NOT happen is the list
    // going empty or the accessors starting to return sentinels.
    let n2 = rs_host::host_npc_count(h);
    assert!(n2 > 0, "the npc list went empty after stepping");
    assert_ne!(rs_host::host_npc_field(h, 0, F_HP), HOST_FIELD_UNKNOWN);

    rs_host::host_free(h);
}
