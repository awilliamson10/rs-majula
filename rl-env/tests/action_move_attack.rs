use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;

fn hold() -> MultiAction {
    MultiAction { move_dx: 0, move_dz: 0, attack: AttackIntent::Hold, prayer: 0, eat: false, equip: 0, spec: false }
}

#[test]
fn move_action_relocates_player() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let start = h.engine.get_player(a).unwrap().player.pathing.coord;
    let mut act = hold(); act.move_dx = 3; // walk +3 tiles east
    for _ in 0..6 { h.apply_actions(a, b, &act); h.cycle(); }
    let end = h.engine.get_player(a).unwrap().player.pathing.coord;
    assert!(end.x() > start.x(), "player moved east via MoveGameClick");
}

#[test]
fn attack_action_deals_damage() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let hp0 = h.engine.get_player(b).unwrap().player.stats.levels[3];
    let mut act = hold(); act.attack = AttackIntent::Engage;
    let mut dmg = false;
    for _ in 0..40 {
        h.apply_actions(a, b, &act);
        h.cycle();
        if h.engine.get_player(b).unwrap().player.stats.levels[3] < hp0 { dmg = true; break; }
    }
    assert!(dmg, "engaging attack reduces opponent HP");
}
