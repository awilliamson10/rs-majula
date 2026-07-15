use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;
use rl_env::observe as ob;

fn scenario() -> Scenario {
    Scenario::load(concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron")).unwrap()
}
fn engage() -> MultiAction {
    MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Engage,
                  prayer: 0, eat: false, equip: 0, spec: false }
}

#[test]
fn obs_len_and_stride_are_exact() {
    assert_eq!(ob::OBS_LEN, 20, "OBS_LEN must be exactly 20 (16 base + 4 derived)");
    assert_eq!(rl_env::batch::BatchEnv::OBS_STRIDE, 26, "OBS_STRIDE must be exactly 26 (20 obs + 6 mask)");
}

#[test]
fn food_remaining_is_positive_at_spawn_and_drops_after_eating() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());
    let (v0, _) = h.observe(a, b);
    let f0 = v0[ob::IDX_FOOD_REMAINING];
    assert!(f0 > 0.0, "food_remaining should be > 0 at spawn (mirror_melee gives 10 sharks)");

    // Content's own guard (`%eat_delay >= map_clock`,
    // content/274/scripts/player/scripts/consumption/consume.rs2:118) blocks
    // eating exactly at clock 0, since the varp defaults to 0 and `0 >= 0`
    // is true -- a genuine content boundary condition on the harness's boot
    // tick, not a formula bug (same boundary `obs_timers.rs`'s
    // `eat_cooldown_is_nonzero_after_eating` documents/works around). Advance
    // one idle tick first so the eat lands past that boundary.
    let idle = MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Hold,
                             prayer: 0, eat: false, equip: 0, spec: false };
    h.apply_actions(a, b, &idle);
    h.cycle();

    let eat = MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Hold,
                            prayer: 0, eat: true, equip: 0, spec: false };
    h.apply_actions(a, b, &eat);
    h.cycle();
    let (v1, _) = h.observe(a, b);
    assert!(v1[ob::IDX_FOOD_REMAINING] < f0, "food_remaining did not drop after eating");
}

#[test]
fn ko_chance_rises_as_opponent_hp_falls() {
    // KO chance must be ~0 at full HP and > 0 once the opponent is low
    // enough for a DDS double-spec to plausibly kill.
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());

    let (v_full, _) = h.observe(a, b);
    assert_eq!(v_full[ob::IDX_SPEC_KO_CHANCE], 0.0, "KO chance should be 0 vs a full-HP opponent");

    // Beat them down, then check KO chance became nonzero.
    let mut rose = false;
    for _ in 0..400 {
        h.apply_actions(a, b, &engage());
        h.cycle();
        if h.player_hp(b) == 0 { break; }
        let (v, _) = h.observe(a, b);
        if v[ob::IDX_SPEC_KO_CHANCE] > 0.0 { rose = true; break; }
    }
    assert!(rose, "KO chance never rose above 0 as the opponent was worn down");
}

#[test]
fn last_hit_magnitudes_are_recorded() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());
    let mut saw_dealt = false;
    for _ in 0..40 {
        h.apply_actions(a, b, &engage());
        h.apply_actions(b, a, &engage());
        h.cycle();
        let _ = h.step_reward_pair(a, b, 1.0); // drains hits; last_* must survive this
        let (v, _) = h.observe(a, b);
        if v[ob::IDX_LAST_DEALT] > 0.0 { saw_dealt = true; break; }
    }
    assert!(saw_dealt, "last_dealt_hit never became nonzero across 40 ticks of mutual melee");
}

#[test]
fn no_exact_opponent_hp_anywhere_in_obs() {
    // MISSION-CRITICAL faithfulness: the opponent's exact HP must never appear
    // in the observation vector -- only the coarse bucket. Scan the WHOLE
    // vector (including the new derived features) for the raw value.
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());
    for _ in 0..30 {
        h.apply_actions(a, b, &engage());
        h.cycle();
        let hp = h.player_hp(b);
        if hp == 0 { break; }
        // Only meaningful once HP is off a bucket boundary, not a small int
        // that legitimately appears elsewhere (e.g. 0/1/10), AND not
        // coincidentally equal to A's own exact HP (obs[IDX_SELF_HP] is
        // legitimately A's real HP -- both sides share the same max HP, so
        // early ticks before either side has taken damage would otherwise
        // false-positive on that legitimate self field, not a real leak).
        if hp > 12 && hp % 10 != 0 && hp != h.player_hp(a) {
            let (v, _) = h.observe(a, b);
            for (i, &x) in v.iter().enumerate() {
                assert!(
                    (x - hp as f32).abs() > 1e-6,
                    "obs[{i}] == exact opponent HP ({hp}) -- faithfulness violated"
                );
            }
        }
    }
}
