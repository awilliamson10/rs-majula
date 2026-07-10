use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent, ResolvedAction};
use rl_env::scenario::Scenario;

/// A deterministic scripted policy: same tick -> same action, no RNG.
fn scripted(tick: u32, pid_is_a: bool) -> MultiAction {
    MultiAction {
        move_dx: if tick % 7 == 0 { if pid_is_a { 1 } else { -1 } } else { 0 },
        move_dz: 0,
        attack: AttackIntent::Engage,
        prayer: 1,
        eat: tick % 13 == 0,
        equip: 0,
        spec: tick == 10 || tick == 40,
    }
}

/// A [`ResolvedAction`] with `pid` replaced by a harness-independent ROLE
/// (`false` = side 0 / "pker" / `a`, `true` = side 1 / "opponent" / `b`).
///
/// Raw engine-assigned pids are NOT comparable across episodes on a reused
/// harness: `EnvHarness::load_scenario` despawns and respawns players every
/// call, but the underlying `PlayerList` slot cursor is not reset, so a
/// reused harness's 2nd episode gets different (higher) pids than its 1st
/// episode -- e.g. episode 1 spawns pids `(1, 2)`, episode 2 spawns pids
/// `(3, 4)`. This is expected slot-allocator behavior (pid identity has no
/// bearing on RNG or combat resolution -- see the passing HP/position
/// trajectory assertions alongside this), not a determinism bug, so the
/// resolved-action log must be compared by ROLE, not raw pid.
type RoleAction = (bool, u32, rl_env::action::ResolvedKind);

fn to_role(log: Vec<ResolvedAction>, a: u16, b: u16) -> Vec<RoleAction> {
    log.into_iter()
        .map(|r| {
            debug_assert!(r.pid == a || r.pid == b, "resolved action pid must be one of this episode's two spawned players");
            (r.pid == b, r.tick, r.kind)
        })
        .collect()
}

type Trajectory = (Vec<u16>, Vec<(i32, i32, i32, i32)>, Vec<RoleAction>);

/// Runs one scripted mirror-melee episode to completion (or 200 ticks) on a
/// GIVEN harness -- i.e. does NOT boot a fresh `EnvHarness` itself, so the
/// caller controls whether `h` is freshly booted or reused across multiple
/// calls. This is what lets [`reused_harness_episode_is_bit_identical_to_itself`]
/// and [`reused_harness_episode_matches_fresh_boot`] drive the SAME harness
/// through a second episode and compare it against either the harness's own
/// first episode or an entirely fresh boot's episode.
fn run_on(h: &mut EnvHarness, sc: &Scenario) -> Trajectory {
    let (a, b) = h.load_scenario(sc);
    let mut hp_traj = Vec::new();     // (a_hp, b_hp) each tick
    let mut coord_traj = Vec::new();  // (ax, az, bx, bz)
    for t in 0..200u32 {
        h.apply_actions(a, b, &scripted(t, true));
        h.apply_actions(b, a, &scripted(t, false));
        h.cycle();
        let (ah, bh) = (h.player_hp(a), h.player_hp(b));
        hp_traj.push(ah); hp_traj.push(bh);
        let (ca, cb) = (h.engine.get_player(a).map(|p| p.player.pathing.coord),
                        h.engine.get_player(b).map(|p| p.player.pathing.coord));
        let f = |c: Option<rs_grid::CoordGrid>| c.map_or((0,0), |c| (c.x() as i32, c.z() as i32));
        let (ca, cb) = (f(ca), f(cb));
        coord_traj.push((ca.0, ca.1, cb.0, cb.1));
        if h.player_hp(a) == 0 || h.player_hp(b) == 0 { break; }
    }
    let log = to_role(h.drain_recorded(), a, b);
    (hp_traj, coord_traj, log)
}

/// Boots a fresh arena harness (seeded from the scenario) and runs one
/// scripted episode on it.
fn run() -> Trajectory {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    run_on(&mut h, &sc)
}

#[test]
fn fight_is_bit_identical_across_runs() {
    let (h1, c1, l1) = run();
    let (h2, c2, l2) = run();
    assert_eq!(h1, h2, "HP trajectory must be identical");
    assert_eq!(c1, c2, "position trajectory must be identical");
    // Compares the FULL resolved-action log (role, tick, AND kind) -- a
    // divergence in the resolved action's *kind* (e.g. a Move with a
    // different clamped dx/dz, or a Prayer(0) where a Prayer(1) was
    // expected) must fail this test, not just a (pid, tick) mismatch. `pid`
    // is normalized to a harness-independent role by [`to_role`] (see its
    // doc comment) rather than compared raw.
    assert_eq!(l1, l2, "resolved action log must be identical");
    assert!(h1.len() > 10, "the scripted fight actually ran");
}

/// Architecture-critical regression test: episode RNG must not depend on the
/// engine's ABSOLUTE clock. `check_afk` (`rs-engine/src/phases/input.rs`)
/// used to draw from `engine.random` -- the SAME RNG combat uses -- gated on
/// `clock.is_multiple_of(500)`. `load_scenario` reseeds `engine.random` but
/// never resets `engine.clock`, so a REUSED harness's Nth episode starts at
/// a high absolute clock and crosses that 500-tick boundary at a different
/// episode-relative tick than a fresh boot (clock 0) would, consuming a
/// different number of RNG draws and silently diverging an otherwise
/// identical (seed, actions) replay.
///
/// This runs TWO sequential scripted episodes on ONE reused
/// `boot_arena_seeded` harness (same scenario, same scripted actions each
/// time, so the same seed drives both since `load_scenario` reseeds
/// `engine.random` every call) and asserts the two episodes' HP, position,
/// and full resolved-action trajectories are bit-identical. If this ever
/// fails, do NOT weaken the assertion -- it means some other absolute-clock
/// -> RNG (or otherwise non-episode-relative state) coupling exists and
/// needs to be found and gated the same way `check_afk` was.
#[test]
fn reused_harness_episode_is_bit_identical_to_itself() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);

    let (h1, c1, l1) = run_on(&mut h, &sc);
    // `h` is NOT re-booted here -- same harness, same underlying `Engine`,
    // now sitting at whatever (high) absolute clock the first episode left
    // it at. `load_scenario` (called again inside `run_on`) reseeds
    // `engine.random` but -- prior to the arena-mode AFK gate -- left
    // `engine.clock` untouched.
    let (h2, c2, l2) = run_on(&mut h, &sc);

    assert_eq!(h1, h2, "HP trajectory must match across episodes on a reused harness");
    assert_eq!(c1, c2, "position trajectory must match across episodes on a reused harness");
    assert_eq!(l1, l2, "resolved action log must match across episodes on a reused harness");
    assert!(h1.len() > 10, "the scripted fight actually ran");
}

/// Companion to [`reused_harness_episode_is_bit_identical_to_itself`]:
/// confirms a reused-harness episode (the 2nd episode run on one harness)
/// matches a completely FRESH-boot harness's episode trajectory bit-for-bit
/// -- i.e. train (long-lived, reused arena harness) and replay (fresh boot
/// per episode) must reach the same fight for the same (seed, actions).
#[test]
fn reused_harness_episode_matches_fresh_boot() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();

    // Reused harness: burn one throwaway episode first, then run the one we
    // compare, so this harness is at a non-trivial absolute clock.
    let mut reused = EnvHarness::boot_arena_seeded(sc.seed);
    let _throwaway = run_on(&mut reused, &sc);
    let (h_reused, c_reused, l_reused) = run_on(&mut reused, &sc);

    // Fresh boot: a brand-new harness at clock 0, same seed/scenario/actions.
    let (h_fresh, c_fresh, l_fresh) = run();

    assert_eq!(h_reused, h_fresh, "HP trajectory must match a fresh boot's");
    assert_eq!(c_reused, c_fresh, "position trajectory must match a fresh boot's");
    assert_eq!(l_reused, l_fresh, "resolved action log must match a fresh boot's");
    assert!(h_reused.len() > 10, "the scripted fight actually ran");
}
