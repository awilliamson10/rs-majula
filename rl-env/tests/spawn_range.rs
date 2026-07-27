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
        damage_coeff: 0.01, win_bonus: 1.0, death_penalty: 0.1, timeout_penalty: 0.4,
        min_sep: 1, max_sep: 12,
    }
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
/// It needs no combat to happen -- the scenario's 400-tick timeout terminates
/// the episode (and therefore respawns it) either way.
#[test]
fn respawned_duels_also_get_a_fresh_seeded_separation() {
    let mut c = cfg(1000);
    c.num_duels = 1;
    c.min_sep = 6;
    c.max_sep = 8;
    let mut env = BatchEnv::new(c);
    let na = env.num_agents();
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na {
        // [move, attack, prayer, eat, equip, spec]; attack 1 = Engage.
        acts[a * BatchEnv::ACT_STRIDE..a * BatchEnv::ACT_STRIDE + 6]
            .copy_from_slice(&[0, 1, 0, 0, 0, 0]);
    }
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
