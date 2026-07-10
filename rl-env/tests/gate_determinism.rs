use rl_env::EnvHarness;
use rl_env::action::{MultiAction, AttackIntent};
use rl_env::scenario::Scenario;

/// A deterministic scripted policy: same tick -> same action, no RNG.
fn scripted(tick: u32, pid_is_a: bool) -> MultiAction {
    MultiAction {
        move_dx: if tick % 7 == 0 { if pid_is_a { 1 } else { -1 } } else { 0 },
        move_dz: 0,
        attack: AttackIntent::Engage,
        prayer: 1,
        eat: tick % 13 == 0,
        equip: 0,
        spec: tick == 10 || tick == 40,
    }
}

fn run() -> (Vec<u16>, Vec<(i32,i32,i32,i32)>, Vec<(u16,u32)>) {
    let sc = Scenario::load("scenarios/mirror_melee.ron").unwrap();
    let mut h = EnvHarness::boot_arena_seeded(sc.seed);
    let (a, b) = h.load_scenario(&sc);
    let mut hp_traj = Vec::new();     // (a_hp, b_hp) each tick
    let mut coord_traj = Vec::new();  // (ax, az, bx, bz)
    for t in 0..200u32 {
        h.apply_actions(a, b, &scripted(t, true));
        h.apply_actions(b, a, &scripted(t, false));
        h.cycle();
        let (ah, bh) = (h.player_hp(a), h.player_hp(b));
        hp_traj.push(ah); hp_traj.push(bh);
        let (ca, cb) = (h.engine.get_player(a).map(|p| p.player.pathing.coord),
                        h.engine.get_player(b).map(|p| p.player.pathing.coord));
        let f = |c: Option<rs_grid::CoordGrid>| c.map_or((0,0), |c| (c.x() as i32, c.z() as i32));
        let (ca, cb) = (f(ca), f(cb));
        coord_traj.push((ca.0, ca.1, cb.0, cb.1));
        if h.player_hp(a) == 0 || h.player_hp(b) == 0 { break; }
    }
    let log: Vec<(u16,u32)> = h.drain_recorded().iter().map(|r| (r.pid, r.tick)).collect();
    (hp_traj, coord_traj, log)
}

#[test]
fn fight_is_bit_identical_across_runs() {
    let (h1, c1, l1) = run();
    let (h2, c2, l2) = run();
    assert_eq!(h1, h2, "HP trajectory must be identical");
    assert_eq!(c1, c2, "position trajectory must be identical");
    assert_eq!(l1, l2, "resolved action log must be identical");
    assert!(h1.len() > 10, "the scripted fight actually ran");
}
