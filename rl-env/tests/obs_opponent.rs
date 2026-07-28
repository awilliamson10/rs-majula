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
fn weapon_class_is_zero_with_armour_but_no_weapon() {
    // DISCRIMINATING TEST for the armour-scanning bug: `weapon_class` must read
    // the RIGHT-HAND slot only. Armour carries a `category` too (rune_full_helm
    // -> armour_helmet), so an implementation that scans the worn inventory for
    // the first slot with any category would return the HELMET's category here
    // (nonzero) instead of 0.0. The plain "> 0.0 when armed" test cannot tell
    // those two implementations apart -- this one can.
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());

    // Remove the weapon from the opponent's right hand, leaving armour on.
    h.unequip_rhand(b);

    let (v, _) = h.observe(a, b);
    assert_eq!(
        v[ob::IDX_OPP_WEAPON], 0.0,
        "weapon_class must be 0.0 with no right-hand weapon -- a nonzero value \
         means it is reading ARMOUR's category instead of the weapon slot"
    );
}

#[test]
fn weapon_class_distinguishes_scimitar_from_dragon_dagger() {
    // In mirror melee the only information this feature can carry is "the
    // opponent swapped to their DDS" -- i.e. a spec is coming. If both
    // weapons map to the same value (e.g. both clamping to a fixed 1.0
    // under a too-small normalization scale), the feature is a dead
    // constant and that read is lost.
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let (a, b) = h.load_scenario(&scenario());

    // mirror_melee starts both sides worn with a rune_scimitar.
    let (before, _) = h.observe(a, b);
    let scimitar_class = before[ob::IDX_OPP_WEAPON];
    assert!(scimitar_class > 0.0, "sanity: scimitar reading should be nonzero");

    // Opponent (b) wields their dragon dagger via the real equip path --
    // mirror_melee starts it loose in the backpack, and `act.equip = 1`
    // fires the item's own OpHeld "Wield" through the real handler (see
    // `action_eat_equip.rs::equip_moves_weapon_from_backpack_to_worn`).
    let mut equip_act = idle();
    equip_act.equip = 1;
    let mut swapped = false;
    let mut dagger_class = scimitar_class;
    for _ in 0..5 {
        h.apply_actions(a, b, &idle());
        h.apply_actions(b, a, &equip_act);
        h.cycle();
        let (v, _) = h.observe(a, b);
        dagger_class = v[ob::IDX_OPP_WEAPON];
        if dagger_class != scimitar_class { swapped = true; break; }
    }
    assert!(swapped, "opponent's weapon_class never changed after wielding the dragon dagger");
    assert_ne!(
        scimitar_class, dagger_class,
        "scimitar and dragon dagger must map to different weapon_class values -- \
         if they collide (e.g. both clamped to 1.0), the feature can't tell the \
         agent a spec weapon just went on"
    );
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
