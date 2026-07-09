use rl_env::EnvHarness;
use rl_env::scenario::Scenario;

#[test]
fn applies_stats_and_inventory() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    assert!(h.engine.get_player(a).is_some() && h.engine.get_player(b).is_some());
    // strength (index 2) set to 99
    let sa = &h.engine.get_player(a).unwrap().player.stats;
    assert_eq!(sa.levels[2], 99);
    assert_eq!(sa.base_levels[2], 99);

    // Resolve the scenario's declared obj ids from the cache so this test
    // proves the REAL declared loadout ("shark" x10, "dragon_dagger" x1)
    // spawned, not a proxy stack (`shark` is non-stackable in rev-274, so
    // 10 sharks occupy 10 separate slots each with `num == 1` -- a naive
    // `item.num >= 10` check would never see them and was only passing
    // before via a `("coins", 1000)` hack that has since been removed from
    // the scenario file).
    let (cache, _) = rl_env::shared_cache();
    let shark_id = cache
        .objs
        .get_by_debugname("shark")
        .expect("shark obj resolves in rev-274 cache")
        .id;
    let dagger_id = cache
        .objs
        .get_by_debugname("dragon_dagger")
        .expect("dragon_dagger obj resolves in rev-274 cache")
        .id;

    let inv_id = cache
        .invs
        .get_by_debugname("inv")
        .expect("backpack inv debugname resolves")
        .id;
    let backpack = &h.engine.get_player(a).unwrap().player.invs[&inv_id];
    let shark_slots = backpack
        .slots
        .iter()
        .flatten()
        .filter(|it| it.obj == shark_id)
        .count();
    assert_eq!(shark_slots, 10, "backpack has exactly 10 shark slots (non-stackable)");
    let has_dagger = backpack.slots.iter().flatten().any(|it| it.obj == dagger_id);
    assert!(has_dagger, "backpack has the declared dragon_dagger");
}

#[test]
fn unresolved_obj_debugname_panics() {
    let mut sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    sc.sides[0]
        .inventory
        .push(("totally_not_an_item".to_string(), 1));
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        h.load_scenario(&sc);
    }));
    let err = result.expect_err("load_scenario must panic on an unresolved obj debugname");
    let msg = err
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        msg.contains("unresolved obj"),
        "panic message should name the unresolved obj: {msg:?}"
    );
}

#[test]
fn load_is_reproducible() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h1 = EnvHarness::boot_arena_seeded(sc.seed);
    let mut h2 = EnvHarness::boot_arena_seeded(sc.seed);
    let (a1, _) = h1.load_scenario(&sc);
    let (a2, _) = h2.load_scenario(&sc);
    let c1 = h1.engine.get_player(a1).unwrap().player.pathing.coord;
    let c2 = h2.engine.get_player(a2).unwrap().player.pathing.coord;
    assert_eq!((c1.x(), c1.z()), (c2.x(), c2.z()), "seeded jitter is reproducible");
}
