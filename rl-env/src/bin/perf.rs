use rl_env::EnvHarness;
use std::time::Instant;

/// Boot one engine (arena or full world) and run a *sustained* 2-agent duel
/// for `cycles` ticks; return ticks/sec (excluding the one-time cache-pack
/// in boot()/boot_arena()).
fn run_one(cycles: u64, arena: bool) -> f64 {
    let mut env = if arena { EnvHarness::boot_arena() } else { EnvHarness::boot() };
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

/// Usage: `perf [cycles] [procs] [arena|full]`
///   - `cycles` (default 20000): ticks per engine.
///   - `procs` (default 1): number of parallel worker processes (each an
///     isolated engine); >1 also reports aggregate/per-proc scaling.
///   - `arena|full` (default `arena`): `arena` boots via
///     `EnvHarness::boot_arena()` (no static world NPCs -- the training-time
///     mode, ~50k+ ticks/s single-thread under combat load, ~100k idle);
///     `full` boots via `EnvHarness::boot()` (full ~7,300-NPC world,
///     ~0.9-1.4k ticks/s single-thread) for comparison.
/// Worker mode (`--worker <cycles> <arena|full>`): one isolated process = one
/// engine; prints just the ticks/s number (used internally when `procs > 1`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--worker") {
        let cycles: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20_000);
        let arena = args.get(3).map(String::as_str) != Some("full");
        println!("{:.0}", run_one(cycles, arena));
        return;
    }

    let cycles: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20_000);
    let procs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mode = args.get(3).map(String::as_str).unwrap_or("arena");
    let arena = mode != "full";

    // Single-engine throughput (the primary number).
    println!("1 process ({mode}): {:.0} ticks/s", run_one(cycles, arena));

    // Parallel scaling via SEPARATE PROCESSES. rs-pathfinder holds process-global
    // `static mut COLLISION_FLAGS`/`PATHFINDER`, so N engines in one process (threads)
    // would race and corrupt each other's collision/pathing — parallel envs MUST be
    // process-isolated (verified in Task 3 review). Each child gets its own globals.
    if procs > 1 {
        let exe = std::env::current_exe().unwrap();
        let children: Vec<_> = (0..procs)
            .map(|_| {
                std::process::Command::new(&exe)
                    .arg("--worker").arg(cycles.to_string()).arg(mode)
                    .stdout(std::process::Stdio::piped())
                    .spawn().unwrap()
            })
            .collect();
        let mut agg = 0.0;
        for c in children {
            let out = c.wait_with_output().unwrap();
            agg += String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0);
        }
        println!("{procs} processes ({mode}): {agg:.0} ticks/s aggregate ({:.0}/proc avg)", agg / procs as f64);
    }
}
