use rl_env::EnvHarness;
use std::time::Instant;

/// Boot one engine and run a *sustained* 2-agent duel for `cycles` ticks;
/// return ticks/sec (excluding the one-time cache-pack in boot()).
fn run_one(cycles: u64) -> f64 {
    let mut env = EnvHarness::boot();
    let (mut a, mut b) = env.reset_duel();
    env.cycle();
    let t0 = Instant::now();
    for i in 0..cycles {
        // Re-inject each tick so combat is actually sustained (representative
        // per-tick load — pathing/LoS/combat scripts running), matching how a
        // real RL env drives the attacker. The combat script's own action-delay
        // guard prevents double-counting hits.
        env.attack_player(a, b);
        env.cycle();
        if i % 300 == 299 { let (na, nb) = env.reset_duel(); a = na; b = nb; }
    }
    cycles as f64 / t0.elapsed().as_secs_f64()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Worker mode (one isolated process = one engine): print just the number.
    if args.get(1).map(String::as_str) == Some("--worker") {
        let cycles: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20_000);
        println!("{:.0}", run_one(cycles));
        return;
    }

    let cycles: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20_000);
    let procs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);

    // Single-engine throughput (the primary number).
    println!("1 process: {:.0} ticks/s", run_one(cycles));

    // Parallel scaling via SEPARATE PROCESSES. rs-pathfinder holds process-global
    // `static mut COLLISION_FLAGS`/`PATHFINDER`, so N engines in one process (threads)
    // would race and corrupt each other's collision/pathing — parallel envs MUST be
    // process-isolated (verified in Task 3 review). Each child gets its own globals.
    if procs > 1 {
        let exe = std::env::current_exe().unwrap();
        let children: Vec<_> = (0..procs)
            .map(|_| {
                std::process::Command::new(&exe)
                    .arg("--worker").arg(cycles.to_string())
                    .stdout(std::process::Stdio::piped())
                    .spawn().unwrap()
            })
            .collect();
        let mut agg = 0.0;
        for c in children {
            let out = c.wait_with_output().unwrap();
            agg += String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0);
        }
        println!("{procs} processes: {agg:.0} ticks/s aggregate ({:.0}/proc avg)", agg / procs as f64);
    }
}
