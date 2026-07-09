use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;

#[test]
fn full_step_loop_and_recording() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    for t in 0..20 {
        let (_obs_a, _m_a) = h.observe(a, b);
        let (_obs_b, _m_b) = h.observe(b, a);
        let mut act = MultiAction { move_dx:0, move_dz:0, attack:AttackIntent::Engage, prayer:1, eat:false, equip:0, spec:(t==5) };
        h.apply_actions(a, b, &act);
        act.prayer = 1;
        h.apply_actions(b, a, &act);
        h.cycle();
        let _ = h.step_reward(a, b, 0.05);
    }
    let log = h.drain_recorded();
    assert!(!log.is_empty(), "resolved actions were recorded");
    assert!(log.iter().any(|r| matches!(r.kind, rl_env::action::ResolvedKind::Attack)));
}
