use rl_env::EnvHarness;
use rl_env::action::{AttackIntent, HEADICON_PROTECT_MELEE, MultiAction, VARP_SPEC_ENERGY};
use rl_env::scenario::Scenario;

fn hold() -> MultiAction {
    MultiAction { move_dx:0, move_dz:0, attack:AttackIntent::Hold, prayer:0, eat:false, equip:0, spec:false }
}

#[test]
fn protect_melee_sets_overhead() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let mut act = hold(); act.prayer = 1; // protect-melee
    for _ in 0..3 { h.apply_actions(a, b, &act); h.cycle(); }
    let icons = h.engine.get_player(a).unwrap().player.headicons;
    assert!(icons & HEADICON_PROTECT_MELEE != 0, "overhead protect-melee active");
}

/// Toggling `prayer=1` then `prayer=0` should turn the overhead back off --
/// exercises the "current-state-aware" toggle logic (not just "click once
/// and never touch it again").
#[test]
fn protect_melee_can_be_deactivated() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);

    let mut act = hold(); act.prayer = 1;
    for _ in 0..3 { h.apply_actions(a, b, &act); h.cycle(); }
    let icons = h.engine.get_player(a).unwrap().player.headicons;
    assert!(icons & HEADICON_PROTECT_MELEE != 0, "overhead should be active before deactivating");

    act.prayer = 0;
    for _ in 0..3 { h.apply_actions(a, b, &act); h.cycle(); }
    let icons = h.engine.get_player(a).unwrap().player.headicons;
    assert!(icons & HEADICON_PROTECT_MELEE == 0, "overhead protect-melee deactivated");
}

/// Wields the backpack `dragon_dagger` (the mirror scenario's designated
/// spec weapon) first via `act.equip = 1` (Task 7's wiring, which switches
/// the active combat sub-interface to `combat_stabsword` -- see
/// `action::com_special_attack`'s doc comment), then arms and fires the
/// special attack against the opponent and asserts `sa_energy` drops.
#[test]
fn spec_drains_energy_after_firing() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);

    let (cache, _) = rl_env::shared_cache();
    let sa_energy_id = cache.varps.get_by_debugname(VARP_SPEC_ENERGY).unwrap().id;
    let energy_before = h.engine.get_player(a).unwrap().player.vars.get(sa_energy_id).as_int();
    assert!(energy_before > 0, "mirror bot should start with spec energy");

    // equip the dagger (moves it backpack -> worn, switches the combat tab)
    let mut act = hold(); act.equip = 1;
    let worn_id = cache.invs.get_by_debugname("worn").unwrap().id;
    let dagger_id = cache.objs.get_by_debugname("dragon_dagger").unwrap().id;
    let mut equipped = false;
    for _ in 0..5 {
        h.apply_actions(a, b, &act);
        h.cycle();
        let worn_has_dagger = h.engine.get_player(a).unwrap().player.invs.get(&worn_id)
            .map(|inv| inv.slots.iter().flatten().any(|it| it.obj == dagger_id))
            .unwrap_or(false);
        if worn_has_dagger { equipped = true; break; }
    }
    assert!(equipped, "the dagger should be wielded before arming spec");

    // Arm spec (`act.spec = true`, a raw click on
    // `combat_stabsword:specbar`'s `@toggle_sa;` -- unlike `prayer`, `spec`
    // has no current-state guard, so holding `true` across multiple ticks
    // would toggle it back off every other tick; a caller wants exactly one
    // armed tick per special) together with `Engage` so it fires right
    // away: the mirror scenario spawns the two sides one tile apart, so
    // the attack range check passes immediately and the special-attack
    // trigger (`[label,pvp_special_attack]` ->
    // `[label,pvp_dragon_dagger_sa]`) runs to completion within this same
    // tick -- i.e. by the time `h.cycle()` returns, `sa_attack` is already
    // back to `false` (consumed) and `sa_energy` has already dropped.
    let mut act = hold();
    act.spec = true;
    act.attack = AttackIntent::Engage;
    let mut energy_dropped = false;
    for _ in 0..10 {
        h.apply_actions(a, b, &act);
        h.cycle();
        let energy_now = h.engine.get_player(a).unwrap().player.vars.get(sa_energy_id).as_int();
        if energy_now < energy_before { energy_dropped = true; break; }
        act.spec = false; // one-tick click; don't re-toggle on later iterations
    }
    assert!(energy_dropped, "special attack energy should drop after firing");
}
