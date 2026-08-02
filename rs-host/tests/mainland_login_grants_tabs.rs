//! The fix for G1c Task 1's finding, at the tightest possible level: engine
//! truth, no client, no FFI.
//!
//! ★ ONE ENGINE PER PROCESS — this binary boots exactly one. It does not go
//! through `host_new_at` (whose handle is opaque) because the assertion is
//! about `player.tabs`, which only the `Engine` can answer.
//!
//! ★★ WHAT WENT WRONG, AND WHY THIS TEST IS SHAPED THIS WAY.
//! `Engine::accept_login` used to take no coordinate: every player was built at
//! `active_player.rs`'s hardcoded `CoordGrid::new(3094, 0, 3106) // Tutorial
//! island` and `on_login()` fired the `[login,_]` trigger THERE, with
//! `spawn_player_tapped` teleporting them afterwards. So
//! `content/274/scripts/login_logout/login.rs2:81`
//!
//! ```text
//! if (%tutorial < ^tutorial_complete & ~in_tutorial_island(coord) = true) {
//!     @start_tutorial;
//! }
//! ~initalltabs;
//! ```
//!
//! always saw a tutorial coordinate, always JUMPED (`@`, so control never
//! returns) to `start_tutorial`, and `~initalltabs` — the only thing in the
//! game that grants the sidebar tabs — was unreachable at every spawn.
//!
//! The observable cost was silent: `client-host/src/state.ts` derives
//! `inventoryComId` from the granted tab and returns -1 when there is none, so
//! every mainland observation carried an EMPTY INVENTORY and nothing errored.
//!
//! ★ The assertions below are on `player.tabs` rather than on the packet
//! stream deliberately. A test that only checked "some IfSetTab bytes went out"
//! would pass on the broken engine too — `start_tutorial` sends thirteen of
//! them, all revocations.

use rl_env::EnvHarness;
use rs_grid::CoordGrid;

/// Lumbridge. Outside `~in_tutorial_island`
/// (`content/274/scripts/tutorial/scripts/util.rs2`: x 3053..=3156,
/// z 3056..=3136 on levels 0..=3, plus the underground x 3072..=3118,
/// z 9492..=9535).
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

/// `^tab_inventory`, and the one whose absence `state.ts` reports as -1.
const TAB_INVENTORY: usize = 3;
/// `^tab_wornitems`.
const TAB_WORNITEMS: usize = 4;
/// `^tab_skills`, granted by `~initalltabs` and nulled by `start_tutorial`.
const TAB_SKILLS: usize = 1;

/// What `if_settab(null, ...)` stores: the client maps 65535 to -1.
const NULL_TAB: u16 = 65535;

#[test]
fn logging_in_at_lumbridge_runs_initalltabs_and_grants_the_sidebar() {
    let (x, level, z) = LUMBRIDGE;
    let mut env = EnvHarness::boot_seeded(4242);
    let (pid, _rx) = env.engine.spawn_player_tapped("agent", CoordGrid::new(x, level, z));

    let p = env.engine.get_player(pid).expect("spawned player");
    let tabs = p.player.tabs;

    // The player is where we asked, and the login script ran there.
    assert_eq!(p.player.pathing.coord.x(), x);
    assert_eq!(p.player.pathing.coord.z(), z);

    // ★ THE FIX'S WHOLE POINT. Before it, every one of these was `Some(65535)`.
    for (name, tab) in [
        ("inventory", TAB_INVENTORY),
        ("wornitems", TAB_WORNITEMS),
        ("skills", TAB_SKILLS),
    ] {
        let got = tabs[tab];
        assert!(
            matches!(got, Some(com) if com != NULL_TAB),
            "tab {name} ({tab}) was not granted at a mainland login: {got:?}. \
             `~initalltabs` did not run, so `login.rs2:81` still jumped to \
             `start_tutorial` — the player logged in on Tutorial Island."
        );
    }

    // ★ AND `%tutorial` STAYS 0, which is the second half of the same fix.
    // `start_tutorial` is what opens `player_kit`; closing that modal fires
    // `[if_close,player_kit]` -> `[queue,tutorial_designed_character]` ->
    // `%tutorial = 1`. A mainland login that never runs `start_tutorial` never
    // opens the modal, so a Lumbridge account is genuinely untouched.
    assert_eq!(env.try_player_varp(pid, "tutorial"), Some(0));

    // Ten live ticks: a queued script that advanced `%tutorial` or revoked a
    // tab a tick later would otherwise pass everything above.
    for _ in 0..10 {
        env.engine.cycle();
    }
    let p = env.engine.get_player(pid).expect("player still logged in");
    assert!(matches!(p.player.tabs[TAB_INVENTORY], Some(com) if com != NULL_TAB));
    assert_eq!(env.try_player_varp(pid, "tutorial"), Some(0));
}
