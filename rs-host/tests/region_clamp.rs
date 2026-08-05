//! The build-area region clamp: `host_set_region` / `host_clear_region`.
//!
//! ★★ ONE ENGINE PER PROCESS: this binary calls a constructor exactly once
//! (`rs-pathfinder` keeps `COLLISION_FLAGS` process-global and `host_new_at`
//! aborts on a second call).
//!
//! ★★ WHAT THIS TEST DOES AND DOES NOT PROVE — read this before trusting it.
//!
//! It proves the clamp does what it says to `BuildArea::mapsquares`: the set
//! shrinks to the region and CLEARING it restores the old behaviour. It does
//! NOT prove anything about what the client renders, and in rev 274 it cannot,
//! because `mapsquares` never reaches the client at all:
//!
//!   * `ActivePlayer::rebuild_normal` puts `mapsquares` on the wire only under
//!     `#[cfg(rev = "225")]`; the `since_244` arm — which is the one that
//!     compiles here — sends `RebuildNormal { zone_x, zone_z }` and nothing
//!     else (`rs-engine/src/active_player.rs:880-907`).
//!   * The only other reader in the workspace is
//!     `handlers/rebuild_get_maps.rs:113`, and the rev-274 client never sends
//!     `REBUILD_GET_MAPS` (opcode 150) — grep `vendor/Client-TS/src` for it:
//!     zero hits, as for `DATA_LAND`/`DATA_LOC`. It loads terrain straight out
//!     of on-demand archive 3 instead (`Client.ts:6845-6870`).
//!
//! So on this revision the clamp is a correct change to a channel the client
//! does not listen to. Kept because it is the right lever on revs <= 245.2 and
//! because the accessor below is the cheapest way to see the build area at all;
//! see `.superpowers/sdd/2026-08-05-g2-pixels/task-2-report.md`.

/// Lumbridge sits in mapsquare (50, 50). 3222 >> 6 == 50, 3218 >> 6 == 50.
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);
const MS: (u16, u16, u16, u16) = (50, 50, 50, 50);

/// ★ MORE THAN FOUR ZONES (32 tiles), or `BuildArea::needs_rebuild` is false and
/// `rebuild_normal` early-returns — the mapsquare set would simply not be
/// recomputed and the test would be reading a number from before the clamp.
/// 40 tiles is 5 zones.
const FAR: u16 = 40;

/// Ticks to let the info phase run its `rebuild_normal(false)`. It happens on
/// the first cycle after the move; the rest is slack.
const SETTLE: u32 = 20;

fn settle(h: *mut std::ffi::c_void) {
    for _ in 0..SETTLE {
        rs_host::host_step(h);
    }
}

#[test]
fn a_clamped_build_area_keeps_only_the_region() {
    let (x, level, z) = LUMBRIDGE;
    let h = rs_host::host_new_at(4242, x, level, z);

    // Unclamped first, so the test proves the clamp CHANGES something rather
    // than asserting against a number that might always have been small.
    let before = rs_host::host_mapsquare_count(h);
    assert!(
        before > 1,
        "an unclamped build area should span several mapsquares, got {before}"
    );

    assert_eq!(rs_host::host_set_region(h, MS.0, MS.1, MS.2, MS.3), rs_host::HOST_REGION_OK);
    // Force a rebuild: the guard only fires on a >4 zone move.
    assert_eq!(rs_host::host_teleport(h, x + FAR, level, z + FAR), rs_host::HOST_TELEPORT_OK);
    settle(h);
    // ★ ASSERTED AT THE FAR COORDINATE TOO, not only after the trip home. The
    // player is standing 40 tiles outside Lumbridge's centre here and its
    // unclamped window straddles mapsquare 51 in both axes; a clamp that only
    // worked when the player happened to be at the region's middle would pass a
    // test that looked at the end state alone.
    assert_eq!(
        rs_host::host_mapsquare_count(h),
        1,
        "clamped to one mapsquare while standing near its edge"
    );

    assert_eq!(rs_host::host_teleport(h, x, level, z), rs_host::HOST_TELEPORT_OK);
    settle(h);
    assert_eq!(rs_host::host_mapsquare_count(h), 1, "clamped to one mapsquare");

    // ★ And clearing it restores the original behaviour — otherwise the clamp
    // is a one-way door and every other consumer of this engine is affected.
    rs_host::host_clear_region(h);
    assert_eq!(rs_host::host_teleport(h, x + FAR, level, z + FAR), rs_host::HOST_TELEPORT_OK);
    settle(h);
    let cleared = rs_host::host_mapsquare_count(h);
    assert!(
        cleared > 1,
        "clearing the region must restore the unclamped sweep, got {cleared}"
    );

    // -- the argument checks, on the same live handle ---------------------------
    // ★ A REJECTED CALL MUST NOT LEAVE A REGION BEHIND. `host_set_region` that
    // stored its arguments before validating them would clamp the world to
    // nonsense and still report an error nobody reads.
    assert_eq!(
        rs_host::host_set_region(h, 51, 50, 50, 50),
        rs_host::HOST_REGION_INVERTED,
        "mx0 > mx1 is not an empty region, it is a caller error"
    );
    assert_eq!(
        rs_host::host_set_region(h, 50, 51, 50, 50),
        rs_host::HOST_REGION_INVERTED
    );
    assert_eq!(
        rs_host::host_set_region(h, 50, 50, 256, 50),
        rs_host::HOST_REGION_OUT_OF_RANGE,
        "a mapsquare coordinate is one byte — 256 would be masked into 0 by the (mx << 8) | mz key"
    );
    assert_eq!(
        rs_host::host_set_region(h, 50, 50, 50, 256),
        rs_host::HOST_REGION_OUT_OF_RANGE
    );
    // ★ Back to the coordinate `before` was measured at, so the comparison is
    // against a number this very handle produced at this very tile rather than
    // against `cleared`, which was taken 40 tiles away where the unclamped
    // window straddles a different set of mapsquares.
    assert_eq!(rs_host::host_teleport(h, x, level, z), rs_host::HOST_TELEPORT_OK);
    settle(h);
    assert_eq!(
        rs_host::host_mapsquare_count(h),
        before,
        "a refused host_set_region must leave the build area exactly as it was"
    );

    rs_host::host_free(h);
}
