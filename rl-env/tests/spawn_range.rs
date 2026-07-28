// Seeded engagement-range randomization (Task 6). Side B spawns at a
// Chebyshev separation drawn from the engine RNG in `[min_sep, max_sep]`.
//
// NOTE what is deliberately NOT tested here: reproducibility. Constructing
// two `BatchEnv`s in one process to compare their spawns would violate the
// one-engine-per-process constraint (rs-pathfinder's process-global collision
// state) -- the exact mistake that produced a false-positive determinism gate
// in B.1. It is also redundant: spawn positions feed the observation
// (`IDX_OPP_DX/DZ/DIST`), so the cross-process `determinism_across_processes`
// gate in `tests/batch_step.rs` already proves that a fixed `base_seed`
// reproduces every spawn byte-for-byte.
use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg(seed: u64) -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: 8, base_seed: seed, spot_stride: 32, reward_w: 1.0,
        damage_coeff: 0.005, win_bonus: 1.0, death_penalty: 0.1, timeout_penalty: 0.4,
        min_sep: 1, max_sep: 12,
    }
}

/// One action row per agent: [move, attack, prayer, eat, equip, spec];
/// attack 1 = Engage, so both sides close and fight.
fn engage_rows(na: usize) -> Vec<i32> {
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na {
        acts[a * BatchEnv::ACT_STRIDE..a * BatchEnv::ACT_STRIDE + 6]
            .copy_from_slice(&[0, 1, 0, 0, 0, 0]);
    }
    acts
}

#[test]
fn duels_start_at_varied_separations_within_bounds() {
    let env = BatchEnv::new(cfg(1000));
    let seps = env.duel_separations();
    assert_eq!(seps.len(), 8);
    for &s in &seps {
        assert!((1..=12).contains(&s), "separation {s} outside [1,12]");
    }
    // Not all identical -- that is the entire point of this task.
    let first = seps[0];
    assert!(
        seps.iter().any(|&s| s != first),
        "all 8 duels spawned at the same separation ({first}) -- no range variety"
    );
}

/// The two tests above only exercise `BatchEnv::new`. A `respawn` that kept
/// the old fixed `spot.0 + 1` offset would pass both of them AND the
/// cross-process determinism gate (a constant is perfectly deterministic), so
/// the auto-reset path needs its own check -- otherwise every episode after
/// the first trains at the identical range, which is the exact overfitting
/// this task exists to prevent.
///
/// The band is [6,8], deliberately excluding the old hardcoded separation of
/// 1: under the unfixed `respawn` this asserts `1` is in `6..=8` and FAILS.
/// It needs no particular combat outcome -- a real kill (~113 ticks under this
/// loadout) or, failing that, the scenario's 400-tick timeout terminates the
/// episode (and therefore respawns it) either way, both inside the 450-tick
/// budget.
#[test]
fn respawned_duels_also_get_a_fresh_seeded_separation() {
    let mut c = cfg(1000);
    c.num_duels = 1;
    c.min_sep = 6;
    c.max_sep = 8;
    let mut env = BatchEnv::new(c);
    let na = env.num_agents();
    let acts = engage_rows(na);
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];

    // Run until the duel ends (death, or the scenario's 400-tick timeout);
    // `step` respawns it in place on that same tick.
    let mut reset = false;
    for _ in 0..450 {
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 { reset = true; break; }
    }
    assert!(reset, "duel never terminated in 450 ticks -- the respawn path was never exercised");

    let s = env.duel_separations()[0];
    assert!(
        (6..=8).contains(&s),
        "respawned duel is at separation {s}, outside the configured [6,8] -- \
         respawn is not drawing a fresh offset"
    );
}

/// ★ The complement to the test above, closing the last hole in the
/// auto-reset path. That one proves `respawn` does not reuse the OLD
/// HARDCODED offset; this one proves it does not reuse the FIRST DRAWN one.
///
/// A `respawn` that cached its offset instead of drawing a fresh one stays
/// inside the configured band forever, so it passes every other test in this
/// file AND the cross-process determinism gate (a cached value is perfectly
/// reproducible) -- while delivering ZERO per-episode variety, which is the
/// entire point of this task. Only comparing separations ACROSS respawns
/// catches it.
///
/// Every offset compared here comes from `respawn` (the first is collected on
/// the tick episode 1 ends, i.e. it is episode 2's spawn), so `new`'s draw
/// cannot mask a frozen respawn.
///
/// It asserts on the full `(dx, dz)` OFFSET rather than just the Chebyshev
/// separation: a cached offset is constant in both, but a correct draw ranges
/// over ~`4 * (2 * sep + 1)` offsets versus only 12 separations, so this reads
/// far more evidence per episode and is correspondingly less likely to see a
/// genuine coincidence.
#[test]
fn respawn_draws_a_fresh_offset_every_episode_not_a_cached_one() {
    const EPISODES: usize = 3;
    let mut c = cfg(1000);
    c.num_duels = 1;
    let mut env = BatchEnv::new(c);
    let na = env.num_agents();
    let acts = engage_rows(na);
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];

    // Budget covers EPISODES full-length episodes plus headroom: a mutual
    // melee resolves by a real kill in ~113 ticks, and the scenario's 400-tick
    // timeout is a hard backstop either way, so the duel always terminates and
    // respawns well inside 1400 ticks.
    let mut offsets: Vec<(i32, i32)> = Vec::with_capacity(EPISODES);
    for _ in 0..1400 {
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 {
            // `step` has already respawned the duel on this tick, so these are
            // the NEW episode's spawn tiles.
            let ((ax, az), (bx, bz)) = env.duel_coords(0);
            offsets.push((bx as i32 - ax as i32, bz as i32 - az as i32));
            if offsets.len() == EPISODES { break; }
        }
    }
    assert_eq!(
        offsets.len(), EPISODES,
        "only {} of {EPISODES} respawns happened inside the tick budget", offsets.len()
    );

    for &(dx, dz) in &offsets {
        let sep = dx.abs().max(dz.abs());
        assert!((1..=12).contains(&sep), "respawn offset ({dx},{dz}) -> separation {sep} outside [1,12]");
    }
    let first = offsets[0];
    assert!(
        offsets.iter().any(|&o| o != first),
        "all {EPISODES} respawns reused the identical offset {first:?} -- respawn is \
         caching its draw instead of drawing a fresh one, so every episode after the \
         first trains at the same engagement range"
    );
}

#[test]
fn separations_respect_the_configured_bounds() {
    // A tighter band must be honored exactly.
    let mut c = cfg(7);
    c.min_sep = 4;
    c.max_sep = 5;
    let env = BatchEnv::new(c);
    for &s in &env.duel_separations() {
        assert!((4..=5).contains(&s), "separation {s} outside the configured [4,5]");
    }
}
