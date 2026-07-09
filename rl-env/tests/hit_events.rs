use rl_env::EnvHarness;
use rs_grid::CoordGrid;

#[test]
fn damage_pushes_a_hit_event() {
    let mut h = EnvHarness::boot_arena_seeded(1);
    let a = h.engine.spawn_player("a", CoordGrid::new(3200, 0, 3912));
    let b = h.engine.spawn_player("b", CoordGrid::new(3201, 0, 3912));
    h.buff_melee(a);
    h.buff_melee(b);
    // drive combat until at least one hit lands on b
    h.attack_player(a, b);
    let mut saw = false;
    for _ in 0..30 {
        h.attack_player(a, b); // re-inject (interaction doesn't persist)
        h.cycle();
        if let Some(p) = h.engine.get_player(b) {
            if !p.player.hits.is_empty() { saw = true; break; }
        }
    }
    assert!(saw, "a landed hit must appear in the victim's hits accumulator");
}
