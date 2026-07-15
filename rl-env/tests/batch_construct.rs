use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg(num_duels: usize) -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels,
        base_seed: 1000,
        spot_stride: 32,
        reward_w: 1.0,
        damage_coeff: 0.005,
        win_bonus: 1.0,
        death_penalty: 0.1,
        timeout_penalty: 0.4,
    }
}

#[test]
fn constructs_m_duels_all_alive_and_separated() {
    let m = 4;
    let env = BatchEnv::new(cfg(m));
    assert_eq!(env.num_agents(), 2 * m);
    assert_eq!(env.num_duels(), m);
    // Every spawned player is alive at full loadout HP.
    for a in 0..env.num_agents() {
        assert_eq!(env.agent_hp(a), 99, "agent {a} not spawned at loadout HP");
    }
    // Duel spots are separated by at least `spot_stride` tiles pairwise.
    let spots = env.duel_spots();
    for i in 0..spots.len() {
        for j in (i + 1)..spots.len() {
            let (xi, _, zi) = spots[i];
            let (xj, _, zj) = spots[j];
            let d = (xi as i32 - xj as i32).abs().max((zi as i32 - zj as i32).abs());
            assert!(d >= 32, "duels {i},{j} only {d} tiles apart");
        }
    }
}
