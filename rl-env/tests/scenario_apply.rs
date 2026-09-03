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
fn applies_worn_equipment() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, _) = h.load_scenario(&sc);

    // Resolve the scenario's declared worn obj ids from the cache so this
    // test proves the REAL declared items ("rune_full_helm",
    // "rune_platebody", "rune_platelegs", "rune_scimitar") landed in the
    // "worn" inv at their cache-declared wear slots -- not a loose
    // "worn inv is non-empty" check. Verified valid rev-274 ids (see
    // content/274/pack/obj.pack): rune_full_helm=1163, rune_platebody=1127,
    // rune_platelegs=1079, rune_scimitar=1333.
    let (cache, _) = rl_env::shared_cache();
    let worn_id = cache
        .invs
        .get_by_debugname("worn")
        .expect("worn inv debugname resolves")
        .id;
    let worn = &h.engine.get_player(a).unwrap().player.invs[&worn_id];

    for name in &sc.sides[0].worn {
        let obj = cache
            .objs
            .get_by_debugname(name)
            .unwrap_or_else(|| panic!("{name:?} obj resolves in rev-274 cache"));
        let wearpos = obj
            .wearpos
            .unwrap_or_else(|| panic!("{name:?} (id {}) has a wearpos in rev-274 cache", obj.id));
        let slot = wearpos as usize;
        let item = worn.slots[slot]
            .as_ref()
            .unwrap_or_else(|| panic!("worn slot {slot} ({name:?}) is occupied"));
        assert_eq!(
            item.obj, obj.id,
            "worn slot {slot} holds the declared {name:?} (id {})",
            obj.id
        );
    }
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

/// ★★ Task 6. `stat_index` knew SEVEN of the twenty-one stats -- the combat
/// ones the duel work needed -- so any `XpGain` or `Stat` condition over
/// Woodcutting, Cooking or Fishing was unauthorable.
///
/// ★★ AND THE TRAP IN FIXING IT: `content/274/scripts/player/configs/
/// stat.constant` numbers the skills 1..19 in the interface's order and says
/// `^woodcutting = 18`, but `StatBlock` is indexed by `PlayerStat`
/// (`rs-pack/src/types.rs:952`), where `Woodcutting = 8`. Reading a stat by the
/// constant's number returns a DIFFERENT skill's level, silently.
#[test]
fn stat_index_covers_the_non_combat_skills() {
    use rl_env::scenario::stat_index;

    // The engine's index, not stat.constant's number.
    assert_eq!(stat_index("cooking"), Some(7));
    assert_eq!(stat_index("woodcutting"), Some(8));
    assert_eq!(stat_index("fishing"), Some(10));
    assert_eq!(stat_index("firemaking"), Some(11));
    assert_eq!(stat_index("mining"), Some(14));
    assert_eq!(stat_index("runecraft"), Some(20));

    // The combat stats keep their existing answers.
    assert_eq!(stat_index("attack"), Some(0));
    assert_eq!(stat_index("defence"), Some(1));
    assert_eq!(stat_index("strength"), Some(2));
    assert_eq!(stat_index("hitpoints"), Some(3));
    assert_eq!(stat_index("ranged"), Some(4));
    assert_eq!(stat_index("prayer"), Some(5));
    assert_eq!(stat_index("magic"), Some(6));

    // The two aliases scenarios on disk already use.
    assert_eq!(stat_index("defense"), Some(1), "the alternate spelling still resolves");
    assert_eq!(stat_index("hp"), Some(3), "the alias still resolves");
}

/// ★★ AN UNKNOWN NAME MUST RETURN `None`, NOT ABORT. `PlayerStat::
/// from_config_str` PANICS on an unrecognised string (`types.rs:999`), so
/// `stat_index` has to reject before it calls -- otherwise a typo in a task
/// file kills the process instead of being reported alongside every other
/// unresolved name.
#[test]
fn an_unknown_stat_name_is_none_and_does_not_panic() {
    use rl_env::scenario::stat_index;
    assert_eq!(stat_index("not_a_skill"), None);
    assert_eq!(stat_index(""), None);
    assert_eq!(stat_index("stat18"), Some(18), "the engine's own filler names still resolve");
}
