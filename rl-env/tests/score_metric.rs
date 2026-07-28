use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg() -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: 1, base_seed: 1000, spot_stride: 32, reward_w: 1.0,
        damage_coeff: 0.005, win_bonus: 1.0, death_penalty: 0.1, timeout_penalty: 0.4,
        min_sep: 1, max_sep: 12,
    }
}

#[test]
fn score_is_emitted_on_terminal_and_is_in_range() {
    // What this integration test proves: the score is EMITTED on exactly the
    // terminal steps, is `-1.0` on every non-terminal step, stays in `[0, 1]`,
    // and carries real credit (> 0) rather than being stuck at the trivial
    // 0.0 -- i.e. `fresh_dealt_a` and the death flags actually reach it.
    //
    // What it deliberately does NOT assert: WHICH outcome each episode has.
    // Now that the arena force-logout is gated off
    // (`rs-engine/src/phases/logout.rs`), episodes resolve by real combat, so
    // in this symmetric mirror melee either side may take any given episode --
    // requiring a particular mix of wins (1.0) and graded partials (in (0,1))
    // here would be pinning an engine outcome this test does not control, and
    // would go red on an unrelated combat-RNG change. The SHAPE of each branch
    // (1.0 for a survived kill, `0.99 * frac^2` otherwise) is pinned exactly by
    // the in-crate `episode_score_*` unit tests, and a real clean win is
    // observed end-to-end by `reward_fresh.rs`'s
    // `the_kill_dominates_a_whole_fights_dense_reward` (pacified opponent, so
    // its outcome IS controlled).
    let mut env = BatchEnv::new(cfg());
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na { acts[a * 6..a * 6 + 6].copy_from_slice(&[0, 1, 0, 0, 0, 0]); }

    let mut finished = 0;
    let mut wins = 0;
    let mut graded_partials = 0;
    let mut saw_credit = false;
    for _ in 0..600 {
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 {
            finished += 1;
            let s = scores[0];
            assert!((0.0..=1.0).contains(&s), "score {s} out of [0,1]");
            if s == 1.0 { wins += 1; }
            if s > 0.0 && s < 1.0 { graded_partials += 1; }
            // Any score above the trivial 0.0 proves the metric ran on real
            // episode data -- either the win branch fired (`b_dead && !a_dead`
            // -> 1.0) or the graded formula (`0.99 * frac^2`) got nonzero
            // fresh damage. A score stuck at 0.0 every episode would mean
            // neither the death flags nor `fresh_dealt_a` are reaching it.
            if s > 0.0 { saw_credit = true; }
        } else {
            assert_eq!(scores[0], -1.0, "score must be -1.0 when no episode finished");
        }
    }
    assert!(finished > 0, "no episode finished in 600 ticks");
    assert!(
        saw_credit,
        "every one of {finished} finished episodes scored 0.0 -- neither a win nor any \
         fresh-damage credit is reaching the score ({wins} wins, {graded_partials} graded)"
    );
}

#[test]
fn score_does_not_depend_on_reward_coefficients() {
    // THE property that makes sweeping the reward safe.
    //
    // Varies ALL FOUR swept coefficients -- `death_penalty` and
    // `timeout_penalty` included, because they are applied at the same call
    // site as the score and a leak through either would otherwise go
    // unnoticed. Returns BOTH streams: the per-episode scores (which must
    // match) and the per-step rewards (which must NOT), so the equality below
    // can't pass by the coefficients having quietly had no effect at all.
    let run = |damage_coeff: f32, win_bonus: f32, death_penalty: f32, timeout_penalty: f32|
        -> (Vec<f32>, Vec<f32>) {
        let mut c = cfg();
        c.damage_coeff = damage_coeff;
        c.win_bonus = win_bonus;
        c.death_penalty = death_penalty;
        c.timeout_penalty = timeout_penalty;
        let mut env = BatchEnv::new(c);
        let na = env.num_agents();
        let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
        let mut rew = vec![0.0f32; na];
        let mut done = vec![0.0f32; na];
        let mut scores = vec![-1.0f32; env.num_duels()];
        let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
        for a in 0..na { acts[a * 6..a * 6 + 6].copy_from_slice(&[0, 1, 0, 0, 0, 0]); }
        let mut out = Vec::new();
        let mut rewards = Vec::new();
        for _ in 0..600 {
            env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
            if scores[0] >= 0.0 { out.push(scores[0]); }
            rewards.extend_from_slice(&rew);
        }
        (out, rewards)
    };
    let (a_scores, a_rewards) = run(0.01, 1.0, 0.1, 0.4);
    let (b_scores, b_rewards) = run(0.05, 4.0, 0.7, 1.3);

    // Non-vacuity: `assert_eq!` on two empty vecs passes trivially, which
    // would make this whole test a no-op if the env stopped finishing
    // episodes (or stopped emitting scores at all).
    assert!(
        !a_scores.is_empty(),
        "no episode finished in 600 ticks -- the score-equality assertion below would be vacuous"
    );
    // Positive control: the coefficients we varied MUST actually move the
    // reward stream. Without this, "the scores matched" could just mean the
    // knobs were dead.
    assert_ne!(
        a_rewards, b_rewards,
        "rewards were identical under different reward coefficients -- \
         the coefficients aren't wired up, so the score-invariance below proves nothing"
    );

    assert_eq!(
        a_scores, b_scores,
        "score changed when only the REWARD coefficients changed -- \
         the sweep objective is not reward-independent"
    );
}
