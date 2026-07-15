use rl_env::batch::{BatchConfig, BatchEnv};
use rl_env::EnvHarness;
use rl_env::scenario::Scenario;
use rl_env::observe as ob;

fn cfg(m: usize) -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: m, base_seed: 1000, spot_stride: 32, reward_w: 1.0,
        damage_coeff: 0.005, win_bonus: 1.0, death_penalty: 0.1, timeout_penalty: 0.4,
    }
}

#[test]
fn write_obs_shape_and_mask_columns() {
    let m = 3;
    let env = BatchEnv::new(cfg(m));
    let mut out = vec![0.0f32; env.num_agents() * BatchEnv::OBS_STRIDE];
    env.write_obs(&mut out);
    // Self HP column (index 0) is 99 for every freshly spawned agent.
    for a in 0..env.num_agents() {
        let base = a * BatchEnv::OBS_STRIDE;
        assert_eq!(out[base + ob::IDX_SELF_HP], 99.0, "agent {a} self-hp");
        // Mask columns are all 0/1.
        for c in 20..26 {
            let v = out[base + c];
            assert!(v == 0.0 || v == 1.0, "mask col {c} agent {a} = {v}");
        }
        // move_ok (col 20) is always legal.
        assert_eq!(out[base + 20], 1.0, "move_ok must be 1");
    }
}

#[test]
fn m1_obs_matches_single_harness() {
    // At M=1 the batch's duel-0 side-A obs must equal a hand-built harness
    // observe() of the same spawn, proving the batch adds no obs drift.
    let env = BatchEnv::new(cfg(1));
    let mut out = vec![0.0f32; env.num_agents() * BatchEnv::OBS_STRIDE];
    env.write_obs(&mut out);

    let mut h = EnvHarness::boot_arena_seeded(1000);
    let sc = Scenario::load(
        concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron")).unwrap();
    // Same fixed spot the batch uses for duel 0 (grid offset 0).
    let a = h.spawn_and_equip("pker", rs_grid::CoordGrid::new(3200, 0, 3912), &sc.sides[0]);
    let b = h.spawn_and_equip("opponent", rs_grid::CoordGrid::new(3201, 0, 3912), &sc.sides[1]);
    let (v, _mask) = h.observe(a, b);
    for i in 0..ob::OBS_LEN {
        assert!((out[i] - v[i]).abs() < 1e-6, "obs[{i}] batch={} single={}", out[i], v[i]);
    }
}
