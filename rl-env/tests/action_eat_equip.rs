use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;

fn hold() -> MultiAction {
    MultiAction { move_dx:0, move_dz:0, attack:AttackIntent::Hold, prayer:0, eat:false, equip:0, spec:false }
}

#[test]
fn eat_heals_and_consumes_food() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    // wound `a` first so eating can heal
    h.engine.get_player_mut(a).unwrap().player.stats.levels[3] = 30;
    let food_before: u32 = h.engine.get_player(a).unwrap().player.invs.values()
        .flat_map(|i| i.slots.iter().flatten()).map(|it| it.num).sum();
    let mut act = hold(); act.eat = true;
    let mut healed = false;
    for _ in 0..5 {
        h.apply_actions(a, b, &act);
        h.cycle();
        if h.engine.get_player(a).unwrap().player.stats.levels[3] > 30 { healed = true; break; }
    }
    let food_after: u32 = h.engine.get_player(a).unwrap().player.invs.values()
        .flat_map(|i| i.slots.iter().flatten()).map(|it| it.num).sum();
    assert!(healed, "eating raised HP");
    assert!(food_after < food_before, "eating consumed a food item");
}

/// The mirror_melee scenario spawns each side with a `dragon_dagger` loose
/// in the backpack (not worn). `act.equip = 1` should wield it: the item
/// moves from the backpack ("inv") to the worn slot.
///
/// `dragon_dagger`'s `OpHeld2` ("Wield") is content-gated behind the Lost
/// City (Zanaris) quest -- see `content/274/scripts/levelrequire/scripts/
/// tier60.rs2`: `[opheld2,dragon_dagger] @levelrequire_zanaris_quest_attack(60, last_slot);`,
/// which (`levelrequire.rs2`) checks `%zanaris >= ^zanaris_complete` before
/// falling through to the actual wield logic. A freshly spawned bot hasn't
/// completed any quest, so this test sets that var directly first --
/// otherwise the wield is correctly (and silently, from the caller's
/// perspective -- no error, just no-op) refused by the game's own
/// quest-requirement check, which is not a bug in the eat/equip wiring.
#[test]
fn equip_moves_weapon_from_backpack_to_worn() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);

    let (cache, _) = rl_env::shared_cache();
    let dagger_id = cache.objs.get_by_debugname("dragon_dagger").unwrap().id;
    let worn_id = cache.invs.get_by_debugname("worn").unwrap().id;
    let zanaris_id = cache.varps.get_by_debugname("zanaris").unwrap().id;
    let zanaris_complete = 6; // `^zanaris_complete` (content/274/scripts/general/configs/quest.constant)
    h.engine.get_player_mut(a).unwrap().player.vars.set(zanaris_id, rs_pack::cache::VarValue::Int(zanaris_complete));

    let dagger_in_backpack_before = h.engine.get_player(a).unwrap().player.invs.values()
        .flat_map(|i| i.slots.iter().flatten())
        .any(|it| it.obj == dagger_id);
    assert!(dagger_in_backpack_before, "scenario should start with the dagger in the backpack");

    let mut act = hold();
    act.equip = 1;
    let mut equipped = false;
    for _ in 0..5 {
        h.apply_actions(a, b, &act);
        h.cycle();
        let worn_has_dagger = h.engine.get_player(a).unwrap().player.invs.get(&worn_id)
            .map(|inv| inv.slots.iter().flatten().any(|it| it.obj == dagger_id))
            .unwrap_or(false);
        if worn_has_dagger { equipped = true; break; }
    }
    assert!(equipped, "wielding should move the dagger into the worn inv");
}
