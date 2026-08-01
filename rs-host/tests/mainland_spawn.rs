//! A spawn that is NOT Tutorial Island.
//!
//! ★ ONE ENGINE PER PROCESS: this binary calls the constructor exactly once.
//! `rs-pathfinder` holds process-global `COLLISION_FLAGS`, and `host_new_at`
//! asserts on a second call — so every assertion below has to fit in one boot.
//!
//! ★★ WHY THIS TEST EXISTS AT ALL. Nothing in this harness had ever booted
//! anywhere but `TUTORIAL_SPAWN`, and `host_new` hardcoded it. The specific
//! silent failure this guards against is a spawn that *reports* success while
//! the engine quietly put the player somewhere else — every later assertion in
//! the milestone would then be a test of Tutorial Island wearing a different
//! name, and nothing would report an error.

use std::ffi::CString;

/// Lumbridge, the mainland respawn point.
///
/// ★ Deliberately outside Tutorial Island, whose bounds are engine truth, not
/// folklore: `content/274/scripts/tutorial/scripts/util.rs2`'s
/// `[proc,in_tutorial_island]` is `x 3053..=3156, z 3056..=3136` on levels 0..=3,
/// plus the underground `x 3072..=3118, z 9492..=9535`. That proc is what
/// `login.rs2:81` branches on, so being outside it is precisely the condition
/// that makes this a mainland login rather than a tutorial one.
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

#[test]
fn a_mainland_spawn_is_alive_somewhere_other_than_tutorial_island() {
    let (x, level, z) = LUMBRIDGE;
    let h = rs_host::host_new_at(4242, x, level, z);
    assert!(!h.is_null());

    // The engine must actually place us there — not silently fall back to the
    // tutorial coordinate, and not clamp onto some nearby walkable tile.
    assert_eq!(rs_host::host_player_x(h), x as i32);
    assert_eq!(rs_host::host_player_z(h), z as i32);

    // Non-vacuity for the two assertions above: the coordinate we asked for is
    // not the one `host_new` would have produced, so they cannot both pass by
    // accident on a build that ignored the arguments entirely.
    assert_ne!(
        (x, level, z),
        rl_env::tape::TUTORIAL_SPAWN,
        "this test is only meaningful at a coordinate host_new would NOT have chosen"
    );

    let tut = CString::new("tutorial").unwrap();
    assert_eq!(
        rs_host::host_varp(h, tut.as_ptr()),
        0,
        "a fresh account must be at %tutorial = 0"
    );

    // ★ AND IT STAYS 0 here, which was NOT true before `accept_login` learned to
    // take a spawn coordinate. A player logged in on Tutorial Island runs
    // `start_tutorial`, which opens `player_kit`; closing that modal fires
    // `[if_close,player_kit]` -> `[queue,tutorial_designed_character]` ->
    // `%tutorial = 1`. A mainland login never opens it. See
    // `mainland_login_grants_tabs.rs` for the assertion that pins this after
    // live ticks rather than only at construction.

    // Ten ticks of a live world, so a spawn that aborts on the first cycle
    // fails here rather than in a Bun test twenty minutes later.
    for _ in 0..10 {
        rs_host::host_step(h);
    }
    assert_eq!(
        rs_host::host_player_x(h),
        x as i32,
        "the player should not have wandered"
    );
    assert_eq!(
        rs_host::host_player_z(h),
        z as i32,
        "the player should not have wandered"
    );

    rs_host::host_free(h);
}
