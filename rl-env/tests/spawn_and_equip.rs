use rl_env::EnvHarness;
use rl_env::scenario::Scenario;
use rs_grid::CoordGrid;

#[test]
fn spawn_and_equip_applies_loadout() {
    let mut h = EnvHarness::boot_arena_seeded(1000);
    let sc = Scenario::load(
        concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron")
    ).unwrap();
    let pid = h.spawn_and_equip("pker", CoordGrid::new(3200, 0, 3912), &sc.sides[0]);
    // Loadout sets hitpoints to 99 -> full HP; a bare spawn would be 10.
    assert_eq!(h.player_hp(pid), 99, "loadout hitpoints not applied");
}
