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

// Ignored by default: each `batch_digest` child pays its own ~110s
// world-cache-pack + engine-boot cost in a debug build, and this test spawns
// three of them sequentially (~5-6 min total) -- too slow for the normal
// `cargo test` loop. Run explicitly:
//   cargo test -p rl-env --test batch_step determinism_across_processes -- --ignored --test-threads=1
#[test]
#[ignore]
fn determinism_across_processes() {
    // Two `BatchEnv`s (two `Engine`s) cannot coexist in one process:
    // rs-pathfinder holds process-global `COLLISION_FLAGS`/`PATHFINDER`
    // state, so an in-process "build two engines, compare their streams"
    // test races on that global and can't distinguish a real
    // batch-determinism bug from cross-engine corruption -- confirmed when
    // an earlier version of this test (in-process, two engines) silently
    // suppressed KOs once given a long enough horizon to reach a respawn.
    // See `src/bin/batch_digest.rs`'s doc comment and the "Final-review fix
    // wave" section of task-5-report.md.
    //
    // The only valid way to compare two batches' streams is therefore two
    // SEPARATE OS processes -- which also matches how training actually
    // runs (one process per parallel env). `batch_digest` runs exactly one
    // `BatchEnv` to completion and prints a digest of its whole
    // (obs, reward, done) stream; this test spawns it three times and
    // compares.
    let bin = env!("CARGO_BIN_EXE_batch_digest");
    let run_digest = |seed: u64, ticks: u32| -> String {
        let out = std::process::Command::new(bin)
            .args([seed.to_string(), ticks.to_string()])
            .output()
            .expect("failed to spawn batch_digest");
        assert!(out.status.success(), "batch_digest exited non-zero: {out:?}");
        String::from_utf8(out.stdout).expect("batch_digest stdout not utf8")
    };

    // 250 ticks: single-engine KOs land roughly every ~55 ticks under this
    // loadout, so this window comfortably covers several respawns per
    // duel -- long enough to exercise the respawn RNG-interleaving
    // determinism guarantee. The done_count assertion below is the safety
    // net if that assumption ever drifts.
    let run1 = run_digest(1000, 250); // same seed as run2
    let run2 = run_digest(1000, 250); // separate process, identical args
    let run3 = run_digest(7, 250);    // different seed

    assert_eq!(run1, run2, "same-seed digests diverged across processes -- respawn determinism broken");
    assert_ne!(run1, run3, "different-seed digests matched -- digest isn't actually sensitive to the RNG stream");

    let done_count: u64 = run1
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix("done_count="))
        .expect("batch_digest output missing done_count field")
        .parse()
        .expect("done_count not a valid integer");
    assert!(done_count > 0, "no terminal/respawn fired in 250 ticks -- test didn't cover the auto-reset path");
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
    // `step` drains `harness.recorded` every call (I1 fix), so after 3000
    // steps at most one step's worth of dispatched actions should remain --
    // it must not have grown unbounded with the step count.
    assert!(
        env.recorded_len() <= 2 * env.num_agents(),
        "harness.recorded grew unbounded: {} entries after 3000 steps",
        env.recorded_len()
    );
}
