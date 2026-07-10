use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;

fn engage(target_is_b: bool) -> MultiAction {
    let _ = target_is_b;
    MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Engage,
                  prayer: 0, eat: false, equip: 0, spec: false }
}

#[test]
fn pair_is_symmetric_and_drains_both() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let sc = Scenario::load(
        concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron")
    ).unwrap();
    let (a, b) = h.load_scenario(&sc);

    // Drive a few ticks of mutual melee so both accumulate hits.
    let mut saw_damage = false;
    for _ in 0..30 {
        h.apply_actions(a, b, &engage(true));
        h.apply_actions(b, a, &engage(false));
        h.cycle();
        let (ra, rb) = h.step_reward_pair(a, b, 1.0);
        // Mirror melee, symmetric weight: a's dealt is b's taken and vice
        // versa, so the non-terminal reward is exactly antisymmetric.
        assert!((ra + rb).abs() < 1e-6, "ra={ra} rb={rb} not antisymmetric");
        if ra != 0.0 || rb != 0.0 { saw_damage = true; }
        // Second call in the SAME tick must see an empty accumulator
        // (both were drained), so it returns only terminal bonuses (0 here).
        let (ra2, rb2) = h.step_reward_pair(a, b, 1.0);
        assert_eq!((ra2, rb2), (0.0, 0.0), "hits not drained by first pair call");
    }
    assert!(saw_damage, "no damage observed across 30 ticks of mutual melee");
}
