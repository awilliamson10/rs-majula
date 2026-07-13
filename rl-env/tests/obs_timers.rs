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

fn idle() -> MultiAction {
    MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Hold,
                  prayer: 0, eat: false, equip: 0, spec: false }
}

#[test]
fn attack_cooldown_is_nonzero_after_attacking_and_counts_down() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let sc = scenario();
    let (a, b) = h.load_scenario(&sc);

    // Before any attack, cooldown must be 0 (ready).
    let (v0, _) = h.observe(a, b);
    assert_eq!(v0[ob::IDX_SELF_ATKCD], 0.0, "cooldown should be 0 before attacking");

    // Attack until a swing actually lands a cooldown (rune scimitar attackrate = 4).
    let mut saw_cd = 0.0f32;
    for _ in 0..10 {
        h.apply_actions(a, b, &engage());
        h.apply_actions(b, a, &idle());
        h.cycle();
        let (v, _) = h.observe(a, b);
        if v[ob::IDX_SELF_ATKCD] > 0.0 { saw_cd = v[ob::IDX_SELF_ATKCD]; break; }
    }
    assert!(saw_cd > 0.0, "attack cooldown never became nonzero after attacking");

    // It must COUNT DOWN on subsequent ticks (this is the whole point --
    // the agent must be able to time the next swing).
    h.apply_actions(a, b, &idle());
    h.apply_actions(b, a, &idle());
    h.cycle();
    let (v2, _) = h.observe(a, b);
    assert!(
        v2[ob::IDX_SELF_ATKCD] < saw_cd,
        "cooldown did not count down: {} -> {}", saw_cd, v2[ob::IDX_SELF_ATKCD]
    );
}

#[test]
fn eat_cooldown_is_nonzero_after_eating() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let sc = scenario();
    let (a, b) = h.load_scenario(&sc);

    let (v0, _) = h.observe(a, b);
    assert_eq!(v0[ob::IDX_SELF_EATDELAY], 0.0, "eat delay should be 0 before eating");

    // Content's own guard (`%eat_delay >= map_clock`,
    // content/274/scripts/player/scripts/consumption/consume.rs2:118)
    // blocks eating exactly at clock 0, since the varp defaults to 0 and
    // `0 >= 0` is true -- a genuine content boundary condition on the
    // harness's boot tick, not a formula bug (confirmed by direct varp
    // trace: an eat issued at clock 1 sets `eat_delay = 1 + 2 = 3`, and the
    // exposed cooldown counts down exactly as `eat_delay - clock` on the
    // following tick). Advance one idle tick first so the eat lands past
    // that boundary.
    h.apply_actions(a, b, &idle());
    h.apply_actions(b, a, &idle());
    h.cycle();

    let eat = MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Hold,
                            prayer: 0, eat: true, equip: 0, spec: false };
    h.apply_actions(a, b, &eat);
    h.apply_actions(b, a, &idle());
    h.cycle();

    let (v, _) = h.observe(a, b);
    assert!(v[ob::IDX_SELF_EATDELAY] > 0.0, "eat delay never became nonzero after eating");
}
