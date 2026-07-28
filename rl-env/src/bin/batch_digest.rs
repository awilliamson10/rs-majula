// Permanent test fixture for `tests/batch_step.rs`'s
// `determinism_across_processes`. rs-pathfinder holds process-global
// `COLLISION_FLAGS`/`PATHFINDER` state (see `perf.rs`'s doc comment) -- two
// `Engine`s (i.e. two `BatchEnv`s) cannot coexist in one process without
// racing on that global. That ruled out the original in-process
// `determinism_two_batches_identical_streams` test (it built two engines in
// one process and, once given a long enough horizon to hit a respawn,
// silently suppressed KOs instead of catching a real bug -- see the
// "Final-review fix wave" section of task-5-report.md).
//
// This binary runs exactly ONE `BatchEnv` to completion in its OWN process
// and prints a digest of its entire (obs, reward, done, score) stream. The
// determinism test runs it twice as SEPARATE OS processes with identical
// args and asserts the digests match byte-for-byte -- each child spawns
// exactly one engine, so this is the only valid way to compare two batches'
// streams. This also matches how training actually runs (one process per
// parallel env; see `perf.rs`'s `procs > 1` mode).
//
// Usage (from majula/, release, foreground):
//   cargo run -p rl-env --release --bin batch_digest -- <seed> <ticks>
use rl_env::batch::{BatchConfig, BatchEnv};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let ticks: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(280);

    let mut env = BatchEnv::new(BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: 2,
        base_seed: seed,
        spot_stride: 32,
        reward_w: 1.0,
        damage_coeff: 0.005,
        win_bonus: 1.0,
        death_penalty: 0.1,
        timeout_penalty: 0.4,
        min_sep: 1,
        max_sep: 12,
    });

    let na = env.num_agents();
    // Fixed action stream: every agent engages every tick, every step.
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na {
        acts[a * BatchEnv::ACT_STRIDE..a * BatchEnv::ACT_STRIDE + 6]
            .copy_from_slice(&[0, 1, 0, 0, 0, 0]);
    }
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];

    // Fold the whole stream into a few f64 accumulators. Both a plain sum
    // and a position-weighted sum are kept so a reordering of values (not
    // just a changed value) is detectable.
    let mut obs_sum = 0.0f64;
    let mut obs_wsum = 0.0f64;
    let mut rew_sum = 0.0f64;
    let mut rew_wsum = 0.0f64;
    let mut done_sum = 0.0f64;
    let mut done_wsum = 0.0f64;
    let mut done_count: u64 = 0;
    // The score stream is folded in too, so the gate also covers the
    // `fresh_dealt_a` accumulator and its per-episode reset -- both of which
    // are carried across respawns and are invisible in obs/rew/done. The
    // `-1.0` no-episode-finished sentinel is included (not skipped): every
    // process runs this identical code, so any consistent treatment is fine,
    // and folding it makes a shifted terminal STEP detectable too, not just a
    // changed score value.
    let mut score_sum = 0.0f64;
    let mut score_wsum = 0.0f64;
    // Sentinels excluded here, so a drift confined to the graded values can't
    // be masked by a compensating change in how many sentinels were summed.
    let mut score_finished_sum = 0.0f64;
    let mut score_count: u64 = 0;
    let mut pos: u64 = 0;

    for _ in 0..ticks {
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        for &v in obs.iter() {
            pos += 1;
            obs_sum += v as f64;
            obs_wsum += v as f64 * pos as f64;
        }
        for &v in rew.iter() {
            pos += 1;
            rew_sum += v as f64;
            rew_wsum += v as f64 * pos as f64;
        }
        for &v in done.iter() {
            pos += 1;
            done_sum += v as f64;
            done_wsum += v as f64 * pos as f64;
            if v == 1.0 { done_count += 1; }
        }
        for &v in scores.iter() {
            pos += 1;
            score_sum += v as f64;
            score_wsum += v as f64 * pos as f64;
            if v >= 0.0 {
                score_finished_sum += v as f64;
                score_count += 1;
            }
        }
    }

    println!(
        "seed={seed} ticks={ticks} obs_sum={obs_sum:.10e} obs_wsum={obs_wsum:.10e} \
         rew_sum={rew_sum:.10e} rew_wsum={rew_wsum:.10e} done_sum={done_sum:.10e} \
         done_wsum={done_wsum:.10e} done_count={done_count} \
         score_sum={score_sum:.10e} score_wsum={score_wsum:.10e} \
         score_finished_sum={score_finished_sum:.10e} score_count={score_count}"
    );
}
