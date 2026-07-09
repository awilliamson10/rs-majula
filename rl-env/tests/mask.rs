use rl_env::EnvHarness;
use rl_env::action::VARP_SPEC_ENERGY;
use rl_env::scenario::Scenario;
use rs_pack::cache::VarValue;

#[test]
fn eat_masked_when_no_food() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, _) = h.load_scenario(&sc);
    assert!(h.legal_mask(a).eat_ok, "has food -> eat legal");
    // strip food: clear the backpack inv
    for inv in h.engine.get_player_mut(a).unwrap().player.invs.values_mut() {
        for s in inv.slots.iter_mut() { *s = None; }
    }
    assert!(!h.legal_mask(a).eat_ok, "no food -> eat illegal");
}

/// `mirror_melee.ron` starts each side with a backpack `dragon_dagger`
/// (`iop` contains `"Wield"`), so `equip_ok` should be true out of the box.
/// Stripping the backpack (same mechanism as `eat_masked_when_no_food`)
/// should flip it to false.
#[test]
fn equip_masked_when_no_wieldable() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, _) = h.load_scenario(&sc);
    assert!(h.legal_mask(a).equip_ok, "has a wieldable weapon -> equip legal");
    for inv in h.engine.get_player_mut(a).unwrap().player.invs.values_mut() {
        for s in inv.slots.iter_mut() { *s = None; }
    }
    assert!(!h.legal_mask(a).equip_ok, "no wieldable weapon -> equip illegal");
}

/// `mirror_melee.ron` declares `("sa_energy", 1000)` on each side, so
/// `spec_ok` should be true (>= the dragon dagger's 250-energy cost, on the
/// varp's native 0..1000 scale -- see `daggers.obj`'s `param=sa_energy,250`)
/// right after load. Driving `sa_energy` below 250 should flip it to false
/// -- this locks in the 0..1000 scale (not a 0..100 "percent" reading, which
/// a naive `>= 25` threshold would silently imply).
#[test]
fn spec_masked_below_energy() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, _) = h.load_scenario(&sc);
    assert!(h.legal_mask(a).spec_ok, "full spec energy -> spec legal");

    let (cache, _) = rl_env::shared_cache();
    let varp = cache.varps.get_by_debugname(VARP_SPEC_ENERGY).unwrap();
    let (id, var_type) = (varp.id, varp.var_type);
    h.engine
        .get_player_mut(a)
        .unwrap()
        .set_varp(id, VarValue::from_int(var_type, 100), false);
    assert!(!h.legal_mask(a).spec_ok, "sub-cost spec energy -> spec illegal");
}

/// Unknown/absent player: `attack_ok` (and every other head) should read as
/// illegal rather than panicking.
#[test]
fn mask_false_for_absent_player() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, _) = h.load_scenario(&sc);
    let absent = if a == 0 { 1 } else { 0 };
    let m = h.legal_mask(absent);
    assert!(!m.attack_ok);
    assert!(!m.eat_ok);
    assert!(!m.equip_ok);
    assert!(!m.prayer_ok);
    assert!(!m.spec_ok);
    assert!(m.move_ok, "move_ok is always true regardless of player presence");
}
