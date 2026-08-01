//! Record a tutorial packet tape to a file.
//!
//! Usage: `packet_tape <out_file> <ticks> [seed]`
//!
//! Also the fixture for the cross-process determinism gate: two separate
//! processes at the same seed must produce byte-identical tapes. Determinism
//! can ONLY be compared across processes (`rs-pathfinder` holds process-global
//! collision state).

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "tape.bin".to_string());
    let ticks: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4242);

    let (bytes, tutorial) = rl_env::tape::record_tutorial_tape(ticks, seed);
    let t = rl_env::tape::TapeReader::parse(&bytes).expect("self-parse");
    std::fs::write(&out, &bytes).expect("write tape");

    // ★ `feed_bytes` — the sum of every tick's payload, with the tape's own
    // 20-byte header and 60 per-tick 8-byte headers subtracted out — is what
    // `client-host/test/live.test.ts` actually pins (`tapeFeedBytes === 2175`).
    // `bytes` (the file's total size, 2675 at that pinning) used to be the
    // only number printed here, which makes a reader do 2675 - 500 = 2175 in
    // their head to compare against the test. Printed directly instead: the
    // README's "verify bytes=2675" step is checking the same value the test
    // checks, byte for byte.
    let feed_bytes: usize = t.ticks.iter().map(|tick| tick.bytes.len()).sum();

    println!("tape={out} ticks={ticks} seed={seed} bytes={} feed_bytes={feed_bytes} tutorial={tutorial} digest={:016x}",
             bytes.len(), rl_env::tape::digest(&t));
}
