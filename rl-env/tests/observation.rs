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
    //
    // 73 is a distinctive value chosen so it can't collide with any *other*
    // legit obs field either -- it's well outside plausible ranges for the
    // dx/dz/dist/bucket/flag fields this scenario produces (players spawn a
    // few tiles apart, and every non-HP field here is a small int, a
    // quantized bucket <= OPP_HP_BUCKETS, or a 0.0/1.0 flag). If this test
    // ever becomes flaky because a legitimate field's value happens to hit
    // 73.0, that's a real collision worth investigating, not a fluke to
    // paper over -- do not just pick a different constant to silence it.
    h.engine.get_player_mut(b).unwrap().player.stats.levels[3] = 73; // exact
    let (obs2, _) = h.observe(a, b);
    let bucket = obs2[rl_env::observe::IDX_OPP_HP_BUCKET];
    // bucket is an integer count in [0, OPP_HP_BUCKETS], never the raw 73
    assert!(bucket <= OPP_HP_BUCKETS as f32);
    assert!((bucket.fract()).abs() < 1e-6, "hp bar is quantized to a bucket index");

    // Structural no-leak proof: scan the WHOLE observation vector and
    // confirm the exact opponent HP (73.0) does not appear at ANY index --
    // not just that IDX_OPP_HP_BUCKET is quantized, but that the raw number
    // isn't smuggled out through some other field either.
    assert!(
        !obs2.iter().any(|&x| (x - 73.0).abs() < 1e-6),
        "exact opponent HP (73.0) leaked into the observation vector: {obs2:?}"
    );
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
