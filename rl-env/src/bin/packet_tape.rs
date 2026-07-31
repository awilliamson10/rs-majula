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

    println!("tape={out} ticks={ticks} seed={seed} bytes={} tutorial={tutorial} digest={:016x}",
             bytes.len(), rl_env::tape::digest(&t));
}
