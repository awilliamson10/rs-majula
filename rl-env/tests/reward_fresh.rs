use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg(damage_coeff: f32) -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: 1,
        base_seed: 1000,
        spot_stride: 32,
        reward_w: 1.0,
        damage_coeff,
        win_bonus: 1.0,
        death_penalty: 0.1,
        timeout_penalty: 0.4,
    }
}

fn engage_row(dst: &mut [i32]) { dst.copy_from_slice(&[0, 1, 0, 0, 0, 0]); }
fn eat_row(dst: &mut [i32])    { dst.copy_from_slice(&[0, 0, 0, 1, 0, 0]); }

#[test]
fn damage_the_opponent_eats_back_is_not_paid_twice() {
    // Agent A attacks; agent B eats to heal back up. With RAW dealt-taken, A
    // would be paid again for re-dealing the same HP. With FRESH damage, A is
    // only paid for pushing B below B's lowest-ever HP this episode.
    let mut env = BatchEnv::new(cfg(1.0));
    env.set_agent_auto_retaliate(1, false);
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];

    // Phase 1: A attacks, B just tanks. Accumulate A's reward.
    let mut r_phase1 = 0.0f32;
    for _ in 0..25 {
        engage_row(&mut acts[0..6]);
        acts[6..12].copy_from_slice(&[0, 0, 0, 0, 0, 0]); // B idles
        env.step(&acts, &mut obs, &mut rew, &mut done);
        if done[0] == 1.0 { break; }
        r_phase1 += rew[0];
    }
    let hp_low = env.agent_hp(1);
    assert!(hp_low < 99, "B should have taken damage in phase 1");

    // Phase 2: A holds; B eats back up above its minimum.
    for _ in 0..12 {
        acts[0..6].copy_from_slice(&[0, 0, 0, 0, 0, 0]); // A idles
        eat_row(&mut acts[6..12]);                        // B eats
        env.step(&acts, &mut obs, &mut rew, &mut done);
        if done[0] == 1.0 { break; }
    }
    let hp_healed = env.agent_hp(1);
    assert!(hp_healed > hp_low, "B should have healed (got {hp_low} -> {hp_healed})");

    // Phase 3: A re-deals the damage B just healed back. Under FRESH-damage
    // accounting this must pay (close to) NOTHING, because it does not push B
    // below B's episode minimum.
    let mut r_phase3 = 0.0f32;
    for _ in 0..10 {
        engage_row(&mut acts[0..6]);
        acts[6..12].copy_from_slice(&[0, 0, 0, 0, 0, 0]);
        env.step(&acts, &mut obs, &mut rew, &mut done);
        if done[0] == 1.0 { break; }
        r_phase3 += rew[0];
        if env.agent_hp(1) <= hp_low { break; } // stop once we're back at fresh ground
    }

    assert!(
        r_phase3 < r_phase1 * 0.5,
        "re-dealing healed-back damage paid {r_phase3} vs {r_phase1} in phase 1 -- \
         the healing farm is still open"
    );
}

#[test]
fn the_kill_dominates_a_whole_fights_dense_reward() {
    // The scaling intent, MEASURED over a real fight rather than asserted
    // between constants. PuffeRL clamps each step's reward to [-1,1], so the
    // balance must come from the COEFFICIENTS: the terminal win must outweigh
    // everything the dense damage term pays across an entire episode.
    // (Otherwise we train a poker, not a killer.)
    let mut env = BatchEnv::new(cfg(0.005)); // damage_coeff 0.005, win_bonus 1.0
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na { engage_row(&mut acts[a * 6..a * 6 + 6]); }

    // Sum agent 0's DENSE reward up to (but excluding) the terminal step.
    let mut dense_total = 0.0f32;
    let mut terminal_r = 0.0f32;
    for _ in 0..600 {
        env.step(&acts, &mut obs, &mut rew, &mut done);
        if done[0] == 1.0 { terminal_r = rew[0]; break; }
        dense_total += rew[0].abs();
    }

    assert!(terminal_r != 0.0, "fight never terminated in 600 ticks");
    assert!(
        terminal_r.abs() > dense_total,
        "terminal reward ({terminal_r}) does not dominate the fight's cumulative \
         dense reward ({dense_total}) -- this trains a poker, not a killer"
    );
}
