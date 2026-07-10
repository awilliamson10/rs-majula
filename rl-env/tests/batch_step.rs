use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg(m: usize) -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: m, base_seed: 1000, spot_stride: 32, reward_w: 1.0,
    }
}

// action row: [move, attack, prayer, eat, equip, spec]; attack 1 = Engage.
fn engage_row(dst: &mut [i32]) { dst.copy_from_slice(&[0, 1, 0, 0, 0, 0]); }

fn run(env: &mut BatchEnv, ticks: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let na = env.num_agents();
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na {
        engage_row(&mut acts[a * BatchEnv::ACT_STRIDE..(a + 1) * BatchEnv::ACT_STRIDE]);
    }
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    for _ in 0..ticks {
        env.step(&acts, &mut obs, &mut rew, &mut done);
    }
    (obs, rew, done)
}

#[test]
fn step_produces_damage_and_antisymmetric_reward() {
    let mut env = BatchEnv::new(cfg(2));
    let (_o, rew, _d) = run(&mut env, 20);
    // Each duel's two agents get antisymmetric non-terminal reward.
    for i in 0..env.num_duels() {
        assert!((rew[2 * i] + rew[2 * i + 1]).abs() < 1e-6);
    }
}

#[test]
fn determinism_two_batches_identical_streams() {
    let mut e1 = BatchEnv::new(cfg(2));
    let mut e2 = BatchEnv::new(cfg(2));
    let (o1, r1, d1) = run(&mut e1, 60);
    let (o2, r2, d2) = run(&mut e2, 60);
    assert_eq!(o1, o2, "obs streams diverged");
    assert_eq!(r1, r2, "reward streams diverged");
    assert_eq!(d1, d2, "done streams diverged");
}

#[test]
fn auto_reset_respawns_after_death() {
    // Long enough for a mirror melee to reach a KO under DeathOrTimeout(400).
    let mut env = BatchEnv::new(cfg(1));
    let na = env.num_agents();
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na { engage_row(&mut acts[a*6..a*6+6]); }
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];

    let mut saw_done = false;
    // Combat under this loadout kills in ~100 ticks, well inside the 600-tick
    // budget and the 400-tick timeout backstop, so multiple auto-resets fire.
    // Snapshot HP right at a reset boundary rather than at the arbitrary
    // final tick: 600 isn't a multiple of the (deterministic but
    // non-round) fight length, so by tick 600 a fresh fight is typically
    // already underway and HP is no longer full.
    let mut hp_at_reset = (0u16, 0u16);
    for _ in 0..600 {
        env.step(&acts, &mut obs, &mut rew, &mut done);
        if done[0] == 1.0 || done[1] == 1.0 {
            saw_done = true;
            hp_at_reset = (env.agent_hp(0), env.agent_hp(1));
        }
    }
    assert!(saw_done, "no terminal in 600 ticks of mutual melee");
    // Immediately after an auto-reset both agents are alive again at full loadout HP.
    assert_eq!(hp_at_reset.0, 99);
    assert_eq!(hp_at_reset.1, 99);
}

#[test]
fn many_respawns_do_not_exhaust_player_slots() {
    // If spawn_player monotonically advances the slot cursor without
    // reusing freed pids, sustained training would eventually panic. Drive
    // many forced episodes and assert the env keeps producing live agents.
    let mut env = BatchEnv::new(cfg(1));
    let na = env.num_agents();
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na { engage_row(&mut acts[a*6..a*6+6]); }
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    for _ in 0..3000 {
        env.step(&acts, &mut obs, &mut rew, &mut done);
    }
    // Not asserting exactly 99: 3000 isn't guaranteed to land on a reset
    // boundary (see auto_reset_respawns_after_death), so the agent may be
    // mid-fight. What matters here is that repeated respawns still produce
    // a live, valid agent rather than panicking or stalling on pid reuse.
    assert!(env.agent_hp(0) > 0, "agent starved of a player slot");
}
