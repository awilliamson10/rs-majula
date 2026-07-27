use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg() -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: 1, base_seed: 1000, spot_stride: 32, reward_w: 1.0,
        damage_coeff: 0.01, win_bonus: 1.0, death_penalty: 0.1, timeout_penalty: 0.4,
    }
}

#[test]
fn score_is_emitted_on_terminal_and_is_in_range() {
    // The score's WIN path (`== 1.0`) is proven deterministically by the
    // in-crate `episode_score_*` unit tests: under this scenario/seed the
    // mirror melee never organically yields a clean side-A solo kill (it
    // produces double-KOs and side-B wins), so asserting an organic `1.0`
    // here would be asserting an outcome the engine does not deliver. What
    // this integration test proves instead: the score is EMITTED on exactly
    // the terminal steps, is `-1.0` on every non-terminal step, stays in
    // `[0, 1]`, and the graded-partial branch actually fires with real
    // (nonzero, sub-win) fresh-damage credit.
    let mut env = BatchEnv::new(cfg());
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na { acts[a * 6..a * 6 + 6].copy_from_slice(&[0, 1, 0, 0, 0, 0]); }

    let mut finished = 0;
    let mut saw_graded_partial = false;
    for _ in 0..600 {
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 {
            finished += 1;
            let s = scores[0];
            assert!((0.0..=1.0).contains(&s), "score {s} out of [0,1]");
            // A strictly-interior score proves the graded-partial formula
            // (`0.99 * frac^2`) ran on real fresh damage -- not the trivial
            // 0.0 (no damage) nor the 1.0 win shortcut.
            if s > 0.0 && s < 1.0 { saw_graded_partial = true; }
        } else {
            assert_eq!(scores[0], -1.0, "score must be -1.0 when no episode finished");
        }
    }
    assert!(finished > 0, "no episode finished in 600 ticks");
    assert!(
        saw_graded_partial,
        "never saw a graded-partial score in (0,1) -- fresh-damage credit isn't reaching the score"
    );
}

#[test]
fn score_does_not_depend_on_reward_coefficients() {
    // THE property that makes sweeping the reward safe.
    let run = |damage_coeff: f32, win_bonus: f32| -> Vec<f32> {
        let mut c = cfg();
        c.damage_coeff = damage_coeff;
        c.win_bonus = win_bonus;
        let mut env = BatchEnv::new(c);
        let na = env.num_agents();
        let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
        let mut rew = vec![0.0f32; na];
        let mut done = vec![0.0f32; na];
        let mut scores = vec![-1.0f32; env.num_duels()];
        let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
        for a in 0..na { acts[a * 6..a * 6 + 6].copy_from_slice(&[0, 1, 0, 0, 0, 0]); }
        let mut out = Vec::new();
        for _ in 0..600 {
            env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
            if scores[0] >= 0.0 { out.push(scores[0]); }
        }
        out
    };
    assert_eq!(
        run(0.01, 1.0),
        run(0.05, 4.0),
        "score changed when only the REWARD coefficients changed -- \
         the sweep objective is not reward-independent"
    );
}
