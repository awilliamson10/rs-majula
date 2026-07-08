use rl_env::EnvHarness;
use rs_grid::CoordGrid;

#[test]
fn player_damages_npc_in_melee() {
    let mut env = EnvHarness::boot();
    let pcoord = CoordGrid::new(3200, 0, 3912);
    let pid = env.engine.spawn_player("attacker", pcoord);
    env.buff_melee(pid);
    // adjacent tile
    let ncoord = CoordGrid::new(3201, 0, 3912);
    let nid = env
        .engine
        .spawn_npc(/* cow */ 81, ncoord)
        .expect("npc")
        .nid();
    env.cycle(); // build_area / visibility settles
    let hp0 = env.npc_hp(nid);
    env.attack_npc(pid, nid);
    for _ in 0..12 {
        env.cycle();
    }
    let hp1 = env.npc_hp(nid);
    assert!(hp1 < hp0, "npc HP should drop: {hp0} -> {hp1}");
}
