// Layer-1 profiler: run a sustained 2-agent deep-wild duel and aggregate the
// engine's per-phase TickStats to find which tick phase dominates.
//
// Run (release, from majula/):  cargo run --release -q -p rl-env --bin profile -- 20000
use rl_env::EnvHarness;

fn main() {
    let cycles: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20_000);

    let mut env = EnvHarness::boot();
    let (a, b) = env.reset_duel();
    env.cycle();

    // Accumulators for each phase field (wall-ms) + total.
    let mut world = 0.0f64;
    let mut input = 0.0;
    let mut npcs = 0.0;
    let mut players = 0.0;
    let mut zones = 0.0;
    let mut info = 0.0;
    let mut out = 0.0;
    let mut cleanup = 0.0;
    let mut logins = 0.0;
    let mut logouts = 0.0;
    let mut ether = 0.0;
    let mut saves = 0.0;
    let mut autosave = 0.0;
    let mut total = 0.0;
    let mut npc_count = 0usize;
    let mut player_count = 0usize;

    for _ in 0..cycles {
        env.attack_player(a, b); // sustained combat = representative load
        env.cycle();
        let s = env.tick_stats();
        world += s.world;
        input += s.input;
        npcs += s.npcs;
        players += s.players;
        zones += s.zones;
        info += s.info;
        out += s.out;
        cleanup += s.cleanup;
        logins += s.logins;
        logouts += s.logouts;
        ether += s.ether;
        saves += s.saves;
        autosave += s.autosave;
        total += s.total_ms;
        npc_count = s.npc_count;
        player_count = s.player_count;
    }

    let n = cycles as f64;
    let mut rows: Vec<(&str, f64)> = vec![
        ("world", world),
        ("input", input),
        ("npcs", npcs),
        ("players", players),
        ("zones", zones),
        ("info", info),
        ("out", out),
        ("cleanup", cleanup),
        ("logins", logins),
        ("logouts", logouts),
        ("ether", ether),
        ("saves", saves),
        ("autosave", autosave),
    ];
    rows.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());

    let mean_total = total / n;
    println!("=== Per-phase tick profile ({cycles} ticks, {player_count} players, {npc_count} npcs) ===");
    println!("{:<10} {:>10} {:>8}", "phase", "mean_ms", "% total");
    for (name, sum) in &rows {
        let mean = sum / n;
        println!("{:<10} {:>10.4} {:>7.1}%", name, mean, 100.0 * mean / mean_total);
    }
    println!("{:<10} {:>10.4}", "TOTAL", mean_total);
    println!("throughput: {:.0} ticks/s", n / (total / 1000.0));
}
