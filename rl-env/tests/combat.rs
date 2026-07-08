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

#[test]
fn player_damages_player_in_deep_wilderness() {
    let mut env = EnvHarness::boot();
    // Both in the Scorpion Valley multi-combat wilderness zone (deep wild ⇒
    // level-difference check trivially passes; multiway ⇒ in-combat check
    // passes). Confirmed against content/274/maps/multiway.csv block
    // 0_50_61_0_8 (mapsquare 50,61, local zone x0-7,y8-15 covers both tiles)
    // and content/274/scripts/areas/area_wilderness/configs/wilderness_zones.dbrow
    // (main wilderness coord_pair covers z0, x2944-3391, y3520-6399; this
    // tile computes to wilderness level 50).
    let a_coord = CoordGrid::new(3200, 0, 3912);
    let b_coord = CoordGrid::new(3201, 0, 3912); // adjacent
    let a = env.engine.spawn_player("pker", a_coord);
    let b = env.engine.spawn_player("victim", b_coord);
    // Buff BOTH: equal combat levels make pvp_level_check's level-difference
    // gate trivially pass at any wilderness depth, and 99 HP on the victim
    // means it survives long enough (doesn't die/respawn, which would reset
    // HP and mask the result) to show a clear HP drop.
    env.buff_melee(a);
    env.buff_melee(b);
    env.cycle(); // both become visible to each other (build_area)

    let hp0 = env.player_hp(b);
    env.attack_player(a, b);
    for _ in 0..16 {
        env.cycle();
    }
    let hp1 = env.player_hp(b);

    assert!(hp1 < hp0, "★ PvP damage must land: victim HP {hp0} -> {hp1}");
}
