use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;
use rl_env::observe as ob;

fn scenario() -> Scenario {
    Scenario::load(concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron")).unwrap()
}
fn idle() -> MultiAction {
    MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Hold,
                  prayer: 0, eat: false, equip: 0, spec: false }
}
fn engage() -> MultiAction {
    MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Engage,
                  prayer: 0, eat: false, equip: 0, spec: false }
}

#[test]
fn opponent_weapon_class_is_nonzero_when_armed() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());
    // mirror_melee equips a rune_scimitar on both sides.
    let (v, _) = h.observe(a, b);
    assert!(v[ob::IDX_OPP_WEAPON] > 0.0, "opponent weapon class should be nonzero when they wield a scimitar");
}

#[test]
fn opponent_is_attacking_becomes_true_when_they_swing() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());

    let mut saw = false;
    for _ in 0..10 {
        h.apply_actions(a, b, &idle());
        h.apply_actions(b, a, &engage()); // opponent attacks us
        h.cycle();
        let (v, _) = h.observe(a, b);
        if v[ob::IDX_OPP_ISATTACKING] == 1.0 { saw = true; break; }
    }
    assert!(saw, "opponent is-attacking never went true while they were attacking");
}

#[test]
fn opponent_is_moving_becomes_true_when_they_move() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());

    let walk = MultiAction { move_dx: 3, move_dz: 0, attack: AttackIntent::Hold,
                             prayer: 0, eat: false, equip: 0, spec: false };
    let mut saw = false;
    for _ in 0..6 {
        h.apply_actions(a, b, &idle());
        h.apply_actions(b, a, &walk); // opponent walks away
        h.cycle();
        // observe() BEFORE note_positions(): compares the just-cycled coord
        // against last iteration's snapshot, then note_positions() takes a
        // fresh snapshot for the NEXT iteration's observe().
        let (v, _) = h.observe(a, b);
        h.note_positions();
        if v[ob::IDX_OPP_ISMOVING] == 1.0 { saw = true; break; }
    }
    assert!(saw, "opponent is-moving never went true while they were walking");
}
