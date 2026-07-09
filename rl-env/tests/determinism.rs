use rl_env::EnvHarness;

#[test]
fn same_seed_same_rng_stream() {
    let mut a = EnvHarness::boot_arena_seeded(42);
    let mut b = EnvHarness::boot_arena_seeded(42);
    let da: Vec<i32> = (0..16).map(|_| a.engine.random.next_int_bound(1000)).collect();
    let db: Vec<i32> = (0..16).map(|_| b.engine.random.next_int_bound(1000)).collect();
    assert_eq!(da, db, "identical seed must produce identical RNG draws");

    let mut c = EnvHarness::boot_arena_seeded(43);
    let dc: Vec<i32> = (0..16).map(|_| c.engine.random.next_int_bound(1000)).collect();
    assert_ne!(da, dc, "different seed should diverge");
}
