use rl_env::tape::{digest, TapeReader, TapeWriter};

#[test]
fn tape_round_trips_through_bytes() {
    let mut w = TapeWriter::new(1234);
    w.record_tick(0, &[vec![1, 2, 3], vec![4, 5]]);
    w.record_tick(1, &[vec![6]]);
    let bytes = w.finish();

    let t = TapeReader::parse(&bytes).expect("parses");
    assert_eq!(t.seed, 1234);
    assert_eq!(t.ticks.len(), 2);
    assert_eq!(t.ticks[0].tick, 0);
    assert_eq!(t.ticks[0].bytes, vec![1, 2, 3, 4, 5]);
    assert_eq!(t.ticks[1].bytes, vec![6]);
}

#[test]
fn digest_changes_when_any_byte_changes() {
    let mut a = TapeWriter::new(1);
    a.record_tick(0, &[vec![1, 2, 3]]);
    let ta = TapeReader::parse(&a.finish()).unwrap();

    let mut b = TapeWriter::new(1);
    b.record_tick(0, &[vec![1, 2, 4]]);
    let tb = TapeReader::parse(&b.finish()).unwrap();

    assert_ne!(digest(&ta), digest(&tb), "digest must be sensitive to payload");
}

#[test]
fn parse_rejects_a_bad_magic() {
    assert!(TapeReader::parse(b"NOPE0000________").is_err());
}

use rl_env::tape::record_tutorial_tape;

/// The recorded stream must be a real per-tick client feed from a genuinely
/// fresh tutorial account. `%tutorial = 0` is load-bearing: the whole G1
/// benchmark is meaningless if the agent starts mid-tutorial.
#[test]
#[ignore = "boots the full world; run on the desktop with --include-ignored"]
fn recording_a_tutorial_tape_yields_a_fresh_account_and_real_packets() {
    let (tape_bytes, tutorial_varp) = record_tutorial_tape(20, 4242);

    assert_eq!(tutorial_varp, 0, "a fresh spawn must start at %tutorial = 0");

    let t = TapeReader::parse(&tape_bytes).expect("parses");
    assert_eq!(t.ticks.len(), 20);

    let total: usize = t.ticks.iter().map(|x| x.bytes.len()).sum();
    assert!(total > 0, "no outbound bytes were captured");

    // PlayerInfo is sent to every player on every tick, so every tick after
    // the first must carry bytes. A tap that silently detached would leave
    // these empty while the tape still "parsed".
    let empty = t.ticks.iter().filter(|x| x.bytes.is_empty()).count();
    assert_eq!(empty, 0, "{empty} ticks captured zero bytes -- the tap detached");
}

/// Determinism is ONLY comparable across processes -- `rs-pathfinder`'s global
/// collision state makes two engines in one process meaningless. Two separate
/// runs of the recorder at the same seed must agree byte-for-byte.
#[test]
#[ignore = "spawns two full-world subprocesses; run on the desktop"]
fn two_processes_at_the_same_seed_record_identical_tapes() {
    let exe = env!("CARGO_BIN_EXE_packet_tape");
    let run = |path: &str| -> String {
        let out = std::process::Command::new(exe)
            .args([path, "30", "777"])
            .output()
            .expect("run packet_tape");
        assert!(out.status.success(), "recorder failed: {:?}", out);
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        s.split_whitespace()
            .find_map(|w| w.strip_prefix("digest=").map(str::to_string))
            .expect("digest in output")
    };

    let dir = std::env::temp_dir();
    let a = run(dir.join("tape_a.bin").to_str().unwrap());
    let b = run(dir.join("tape_b.bin").to_str().unwrap());

    assert_eq!(a, b, "same seed produced different tapes across processes");

    // Non-vacuity: a different seed must produce a different digest, or the
    // equality above proves nothing.
    let out = std::process::Command::new(exe)
        .args([dir.join("tape_c.bin").to_str().unwrap(), "30", "888"])
        .output()
        .expect("run packet_tape");
    let c = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find_map(|w| w.strip_prefix("digest=").map(str::to_string))
        .expect("digest");
    assert_ne!(a, c, "different seeds produced the same digest -- the seed is inert");
}
