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
    // backpack has 10 sharks somewhere
    let has_food = h.engine.get_player(a).unwrap().player.invs.values()
        .any(|inv| inv.slots.iter().flatten().any(|it| it.num >= 10));
    assert!(has_food, "backpack inventory applied");
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
