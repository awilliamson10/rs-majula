use rl_env::EnvHarness;
use rs_grid::CoordGrid;

#[test]
fn spawns_a_player_at_coord_with_default_hp() {
    let mut env = EnvHarness::boot();
    // Scorpion Valley (deep wild, multi-combat): mapsquare (50,61), local (0,8).
    let coord = CoordGrid::new(3200, 0, 3912);
    let pid = env.engine.spawn_player("bot1", coord);
    env.cycle(); // let login/appearance settle
    let p = env.engine.get_player(pid).expect("player present");
    assert_eq!(p.player.pathing.coord.x(), 3200);
    assert_eq!(p.player.pathing.coord.z(), 3912);
    // Hitpoints stat index = 3; new-player default HP level = 10.
    assert_eq!(p.player.stats.levels[3], 10, "default HP should be 10");
}
