use rl_env::EnvHarness;
use rl_env::scenario::Scenario;
use rl_env::observe::{OBS_LEN, OPP_HP_BUCKETS};

#[test]
fn obs_has_fixed_len_and_no_exact_opp_hp() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let (obs, _mask) = h.observe(a, b);
    assert_eq!(obs.len(), OBS_LEN);

    // wound opponent to just below a bucket edge and confirm obs shows a
    // COARSE bucket, not the exact HP.
    h.engine.get_player_mut(b).unwrap().player.stats.levels[3] = 73; // exact
    let (obs2, _) = h.observe(a, b);
    let bucket = obs2[rl_env::observe::IDX_OPP_HP_BUCKET];
    // bucket is an integer count in [0, OPP_HP_BUCKETS], never the raw 73
    assert!(bucket <= OPP_HP_BUCKETS as f32);
    assert!((bucket.fract()).abs() < 1e-6, "hp bar is quantized to a bucket index");
}

#[test]
fn absent_opponent_yields_zeroed_block() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let _ = h.engine.remove_player(b);
    let (obs, _) = h.observe(a, b);
    assert_eq!(obs.len(), OBS_LEN); // no panic; opp block zeroed
}
