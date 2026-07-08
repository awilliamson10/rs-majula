use rl_env::EnvHarness;

#[test]
fn boots_and_ticks_empty_world() {
    let mut env = EnvHarness::boot_arena();
    let start = env.clock();
    for _ in 0..10 {
        env.cycle();
    }
    assert_eq!(env.clock(), start + 10, "clock should advance one per cycle");
}
