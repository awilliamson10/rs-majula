use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::reward::Outcome;
use rl_env::scenario::{Scenario, Terminal};

fn engage() -> MultiAction {
    MultiAction { move_dx:0, move_dz:0, attack:AttackIntent::Engage, prayer:0, eat:false, equip:0, spec:false }
}

#[test]
fn reward_reads_hit_events_not_hp_delta() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let mut total = 0.0;
    for _ in 0..40 {
        h.apply_actions(a, b, &engage());
        h.cycle();
        total += h.step_reward(a, b, 1.0);
    }
    assert!(total != 0.0, "some net damage was traded and rewarded");
}

#[test]
fn eat_on_damage_tick_does_not_hide_damage() {
    // Manufacture: opponent takes 5 dmg AND we set HP up (simulating an eat)
    // the same step; event-based reward must still count the 5 dealt.
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    // push a synthetic hit onto b, then raise b's HP (as an eat would)
    h.engine.get_player_mut(b).unwrap().player.hits.push(
        rs_entity::player::HitEvent { amount: 5, kind: 0 });
    let hp = h.engine.get_player(b).unwrap().player.stats.levels[3];
    h.engine.get_player_mut(b).unwrap().player.stats.levels[3] = hp + 18; // "ate"
    let r = h.step_reward(a, b, 1.0);
    assert!(r >= 5.0, "5 damage dealt is counted despite same-step healing (got {r})");
}

#[test]
fn death_is_terminal_win() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    h.engine.get_player_mut(b).unwrap().player.stats.levels[3] = 0;
    assert_eq!(h.is_terminal(a, b, &Terminal::Death), Some(Outcome::Win));
}
