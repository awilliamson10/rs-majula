//! The segment boundary, on the ENGINE side of the FFI.
//!
//! ★ ONE ENGINE PER PROCESS: this binary calls the constructor exactly once and
//! every assertion below has to fit in that one boot. `rs-pathfinder` holds
//! process-global `COLLISION_FLAGS` and `host_new_at` asserts on a second call.
//!
//! ★★ WHAT THIS FILE CAN AND CANNOT SEE. The trap `host_teleport` exists for —
//! a hop of 4 zones or fewer leaving the CLIENT rendering the old region — is
//! invisible from here, because there is no client in this process. That half is
//! pinned in `client-host/test/mainland-live.test.ts`, which watches
//! `client.mapBuildBaseX/Z`. What this file pins is everything the engine itself
//! decides: that the player really arrives, that a destination the engine refuses
//! is REPORTED as refused rather than silently ignored, that a coordinate
//! `CoordGrid` would have masked is rejected rather than quietly relocated, and
//! that a teleported player stays put instead of walking back.

use std::ffi::CString;

/// Lumbridge, the mainland respawn point — outside `~in_tutorial_island`, so
/// this is a genuine mainland login (see `mainland_spawn.rs`).
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

/// Varrock, ~206 tiles north of Lumbridge: 25 zones, comfortably past
/// `BuildArea::needs_rebuild`'s `> 4`.
const VARROCK: (u16, u8, u16) = (3210, 0, 3424);

/// One zone north. ★ INSIDE the 4-zone threshold on purpose — the engine moves
/// the player and sends no `REBUILD_NORMAL` at all.
const SHORT_HOP: (u16, u8, u16) = (3222, 0, 3226);

/// A coordinate the world has no mapsquare for, so `rsmod::is_zone_allocated`
/// says no and `ActivePlayer::tele` answers with "Invalid teleport!".
const UNALLOCATED: (u16, u8, u16) = (0, 0, 0);

fn tile(h: *mut std::ffi::c_void) -> (i32, i32) {
    (rs_host::host_player_x(h), rs_host::host_player_z(h))
}

#[test]
fn the_segment_boundary_moves_the_player_and_reports_when_it_cannot() {
    let (lx, llevel, lz) = LUMBRIDGE;
    let h = rs_host::host_new_at(4242, lx, llevel, lz);
    assert!(!h.is_null());
    assert_eq!(tile(h), (lx as i32, lz as i32), "the spawn itself must land");

    // -- refusals, all of which must leave the player exactly where it was ------

    // ★★ THE CASE THAT DISCRIMINATES THE IMPLEMENTATION. A bare
    // `pathing.set_coord(...)` — the shape this task's brief guessed at — would
    // put the player at (0, 0, 0) and return OK. The engine's own teleport path
    // consults `rsmod::is_zone_allocated` first, refuses, and messages the
    // player instead. Without this assertion, "we used the engine's teleport"
    // would be a claim about the source rather than about the behaviour.
    let (ux, ulevel, uz) = UNALLOCATED;
    assert_eq!(
        rs_host::host_teleport(h, ux, ulevel, uz),
        rs_host::HOST_TELEPORT_REFUSED,
        "an unallocated zone must be REPORTED, not silently ignored — the engine \
         answers it with a chat message this side cannot see"
    );
    assert_eq!(
        tile(h),
        (lx as i32, lz as i32),
        "a refused teleport must not have moved anything"
    );

    // ★ `CoordGrid::new` MASKS rather than rejects (`x & 0x3FFF`, `y & 0x3`,
    // `z & 0x3FFF`, rs-grid/src/coord.rs:49), so 20000 would have become 3616 —
    // a real, allocated, perfectly plausible tile ~400 west of Lumbridge. The
    // teleport would have "succeeded" somewhere nobody asked for.
    assert_eq!(20000u16 & 0x3FFF, 3616, "the masking this check exists to prevent");
    assert_eq!(
        rs_host::host_teleport(h, 20000, 0, lz),
        rs_host::HOST_TELEPORT_OUT_OF_RANGE
    );
    // Same for the level: `9 & 0x3` is 1, an ordinary upper floor.
    assert_eq!(9u8 & 0x3, 1);
    assert_eq!(
        rs_host::host_teleport(h, lx, 9, lz),
        rs_host::HOST_TELEPORT_OUT_OF_RANGE
    );
    assert_eq!(
        tile(h),
        (lx as i32, lz as i32),
        "an out-of-range teleport must not have moved anything"
    );

    // -- the short hop: a REAL teleport that will not rebuild the client's scene -

    let (sx, slevel, sz) = SHORT_HOP;
    assert_eq!(rs_host::host_teleport(h, sx, slevel, sz), rs_host::HOST_TELEPORT_OK);
    assert_eq!(tile(h), (sx as i32, sz as i32));
    // Non-vacuity for the client-side negative control: this really is inside the
    // threshold, so the client-side test is measuring the >4-zone rule and not a
    // teleport that failed outright.
    let zones = ((sz >> 3) as i32 - (lz >> 3) as i32).abs();
    assert!(zones <= 4, "SHORT_HOP moved {zones} zones; it must stay inside 4");

    // -- the far teleport ------------------------------------------------------

    let (vx, vlevel, vz) = VARROCK;
    let far = ((vz >> 3) as i32 - (sz >> 3) as i32).abs();
    assert!(far > 4, "VARROCK is only {far} zones away; it must be more than 4");
    assert_eq!(rs_host::host_teleport(h, vx, vlevel, vz), rs_host::HOST_TELEPORT_OK);
    assert_eq!(tile(h), (vx as i32, vz as i32));

    // ★ AND IT STAYS THERE. `Pathing::reset` clears the walk step and the tele
    // flag every tick but NOT `waypoint_index`, so a player teleported mid-walk
    // would resume the old path from the new coordinate — a segment quietly
    // walking back toward the previous region. `host_teleport` clears the
    // waypoints for exactly that reason. (This run was standing still, so what
    // these ticks prove is only that nothing ELSE drags the player off the
    // destination; the in-flight case is exercised through the client in
    // `client-host/src/mainland-once.ts`, where a walk can actually be issued.)
    for _ in 0..10 {
        rs_host::host_step(h);
    }
    assert_eq!(
        tile(h),
        (vx as i32, vz as i32),
        "the player wandered off the teleport destination"
    );

    // The account is still the same fresh mainland one — the teleport is a
    // relocation, not a re-login, so nothing re-ran the `[login,_]` trigger.
    let tut = CString::new("tutorial").unwrap();
    assert_eq!(rs_host::host_varp(h, tut.as_ptr()), 0);

    rs_host::host_free(h);
}
