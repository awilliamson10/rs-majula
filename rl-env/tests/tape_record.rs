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

/// Without hashing `ticks.len()`/each tick's `bytes.len()`, one tick numbered
/// 0 with payload `[1, 0, 0, 0]` and two empty ticks numbered 0 and 1
/// serialize to the IDENTICAL byte stream once `seed` is stripped off
/// (`[0,0,0,0, 1,0,0,0]` either way: tick-number bytes and payload bytes
/// concatenate with no delimiter), so a naive digest collides them. This is a
/// real, checked collision against the pre-fix `digest`, not a hypothetical
/// one -- see the fix-round report for the before/after run that proves it.
#[test]
fn digest_is_sensitive_to_tick_structure_not_just_concatenated_bytes() {
    let mut a = TapeWriter::new(1);
    a.record_tick(0, &[vec![1, 0, 0, 0]]);
    let ta = TapeReader::parse(&a.finish()).unwrap();

    let mut b = TapeWriter::new(1);
    b.record_tick(0, &[]);
    b.record_tick(1, &[]);
    let tb = TapeReader::parse(&b.finish()).unwrap();

    assert_ne!(
        digest(&ta),
        digest(&tb),
        "digest must distinguish tape structure (tick count / per-tick length), not just concatenated bytes"
    );
}

/// A buffer long enough to clear the length guard (>= 20 bytes) but with the
/// wrong magic -- this is the only way to actually exercise the magic
/// comparison. The earlier version of this test used a 16-byte buffer, which
/// is rejected by the `bytes.len() < 20` guard alone: deleting the magic
/// comparison entirely would have left it green.
#[test]
fn parse_rejects_a_bad_magic() {
    let mut bad = b"NOPE0000".to_vec();
    bad.extend_from_slice(&[0u8; 12]); // pad to a valid 20-byte header
    assert!(TapeReader::parse(&bad).is_err());
}

#[test]
fn parse_rejects_a_truncated_buffer() {
    assert!(TapeReader::parse(b"short").is_err());
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

use rl_env::EnvHarness;
use rs_crypto::isaac::Isaac;
use rs_grid::CoordGrid;

/// rev-274 `ServerProt` opcodes (`rs-protocol/.../server_prot.rs`, the
/// `#[cfg(rev = "274")]` block). Rev-scoped -- do not reuse across revs.
const PLAYER_INFO: u8 = 167;
const REBUILD_NORMAL: u8 = 231;

/// `rs-protocol::LoginResponse::SuccessNormal`. NOT ISAAC-encoded -- see the
/// skip below.
const LOGIN_RESPONSE_SUCCESS: u8 = 2;

/// Proves two things the byte-count checks above cannot: (1) the tap
/// captures the LOGIN burst, not just the steady-state per-tick feed --
/// `RebuildNormal` (the map build the client needs before it can render
/// anything) must appear in tick 0, alongside exactly one `PlayerInfo`; and
/// (2) the outbound ISAAC stream is genuinely decodable in lockstep from byte
/// zero with a from-scratch `Isaac::new(&[0; 4])` mirror -- i.e. this is a
/// real, decodable client feed, not bytes that merely happen to have the
/// right length. This also guards `spawn_player_tapped`'s no-re-seat-needed
/// claim: if the encoder and this mirror ever fell out of lockstep, decoded
/// opcodes would be scattered noise and "exactly one PlayerInfo per tick"
/// would fail.
#[test]
#[ignore = "boots the full world; run on the desktop with --include-ignored"]
fn tapped_spawn_yields_a_decodable_login_burst_and_steady_state_stream() {
    let mut env = EnvHarness::boot();
    let (x, level, z) = rl_env::tape::TUTORIAL_SPAWN;
    let (_pid, mut rx) =
        env.engine.spawn_player_tapped("tutorial", CoordGrid::new(x, level, z));

    // `accept_login`'s very FIRST outbox send -- before `on_login()` even
    // runs -- is a raw, NOT-ISAAC-encoded login response
    // (`active.handle.outbox.send(vec![LoginResponse::SuccessNormal as u8,
    // ...])` in `Engine::accept_login`, bypassing `write()`/isaac entirely).
    // A real client reads this fixed handshake reply before the cipher
    // starts. Decoding it as if it were isaac-encoded (an earlier version of
    // this test did exactly that) consumes one spurious `next_int()` and
    // permanently desyncs the mirror -- every opcode after it comes out as
    // scattered noise. Skip it explicitly instead.
    let login_response = rx.try_recv().expect("login response packet");
    assert_eq!(
        login_response[0], LOGIN_RESPONSE_SUCCESS,
        "expected LoginResponse::SuccessNormal (2) as the first raw packet; got {login_response:?}"
    );

    let mut isaac = Isaac::new(&[0; 4]);

    // Tick 0: cycle first, THEN drain -- this is the same discipline
    // `record_tutorial_tape` uses, and it is what actually matters: some of
    // `on_login`'s writes are Immediate-priority (sent the instant
    // `accept_login` runs, before this function's first `cycle()`) and some
    // are Buffered (queued, flushed on the first subsequent `cycle()`).
    // Draining only after `cycle()` bundles both into "tick 0" regardless of
    // which each opcode happens to be, so this test doesn't need to know or
    // care about that split.
    env.engine.cycle();
    let mut tick0_opcodes = Vec::new();
    while let Ok(buf) = rx.try_recv() {
        tick0_opcodes.push((buf[0] as u32).wrapping_sub(isaac.next_int()) as u8);
    }
    assert!(
        tick0_opcodes.contains(&REBUILD_NORMAL),
        "no RebuildNormal in tick 0 -- the login burst was lost. Opcodes: {tick0_opcodes:?}"
    );
    assert_eq!(
        tick0_opcodes.iter().filter(|&&o| o == PLAYER_INFO).count(),
        1,
        "expected exactly one PlayerInfo in tick 0; got {tick0_opcodes:?}"
    );

    // 4 more ticks of pure steady state: exactly one PlayerInfo each. If the
    // isaac mirror had drifted out of lockstep, decoded opcodes would be
    // scattered noise and this count would almost never land on exactly 1.
    for tick in 1..=4 {
        env.engine.cycle();
        let mut count = 0;
        while let Ok(buf) = rx.try_recv() {
            let opcode = (buf[0] as u32).wrapping_sub(isaac.next_int()) as u8;
            if opcode == PLAYER_INFO {
                count += 1;
            }
        }
        assert_eq!(count, 1, "expected exactly one PlayerInfo in tick {tick}; got {count}");
    }
}

/// Determinism is ONLY comparable across processes -- `rs-pathfinder`'s global
/// collision state makes two engines in one process meaningless. Two separate
/// runs of the recorder at the same seed must agree byte-for-byte.
///
/// Self-sufficient: this reads the tapes back off disk and checks (a) the raw
/// files are byte-identical, (b) neither is vacuously empty, so a silently
/// detached tap producing two empty-but-equal tapes cannot pass, and (c) the
/// PARSED TICK PAYLOADS (not the raw files -- see the comment inline) differ
/// under a different seed, which is what actually proves `seed`
/// (`EnvHarness::boot_seeded`) reaches `Engine::random` rather than just
/// landing in the tape's own header field.
#[test]
#[ignore = "spawns two full-world subprocesses; run on the desktop"]
fn two_processes_at_the_same_seed_record_identical_tapes() {
    let exe = env!("CARGO_BIN_EXE_packet_tape");
    let run = |path: &std::path::Path, seed: &str| -> Vec<u8> {
        let out = std::process::Command::new(exe)
            .args([path.to_str().unwrap(), "30", seed])
            .output()
            .expect("run packet_tape");
        assert!(out.status.success(), "recorder failed: {out:?}");
        std::fs::read(path).expect("tape file written")
    };

    // ★ A UNIQUE DIRECTORY, not bare `temp_dir()`. The desktop is shared and
    // runs suites concurrently, so fixed `/tmp/tape_{a,b,c}.bin` paths let one
    // run overwrite the tape another run is about to read back -- which would
    // present as a spurious determinism failure, the single most alarming and
    // least believable way for this suite to go red.
    let dir = std::env::temp_dir().join(format!("cshanty-tape-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the per-process tape directory");
    let bytes_a = run(&dir.join("tape_a.bin"), "777");
    let bytes_b = run(&dir.join("tape_b.bin"), "777");

    assert_eq!(bytes_a, bytes_b, "same seed produced different tapes across processes");

    // Non-vacuity: a tap that silently detached in BOTH subprocesses would
    // also produce byte-identical (empty) tapes and pass the assert above.
    let ta = TapeReader::parse(&bytes_a).expect("parses");
    assert!(!ta.ticks.is_empty(), "tape recorded zero ticks");
    let empty = ta.ticks.iter().filter(|t| t.bytes.is_empty()).count();
    assert_eq!(empty, 0, "{empty} ticks captured zero bytes -- the cross-process gate is vacuous");

    // Non-vacuity across seeds: a different seed must produce a different
    // PACKET STREAM. Comparing whole files (an earlier version of this test
    // did that) is tautological -- `TapeWriter::finish` writes `seed` into
    // the header at offset 8..16, so seeds 777 vs 888 make the files differ
    // by construction whether or not the engine's RNG is touched at all.
    // Comparing only the parsed tick payloads (which exclude the seed field)
    // actually exercises whether `seed` reaches `Engine::random`. Empirically
    // checked (not assumed): even over this idle 30-tick tutorial-spawn
    // window, 27 of 30 ticks differ in payload between seed 777 and seed
    // 888, including differing packet lengths (not just cipher noise on
    // identical-length data) -- static-NPC wander behaviour near the spawn
    // draws from the same seeded `JavaRandom`, so it's a genuine,
    // observable engine effect, not something that needed a longer window.
    let bytes_c = run(&dir.join("tape_c.bin"), "888");
    let tc = TapeReader::parse(&bytes_c).expect("parses");
    let pa: Vec<&Vec<u8>> = ta.ticks.iter().map(|t| &t.bytes).collect();
    let pc: Vec<&Vec<u8>> = tc.ticks.iter().map(|t| &t.bytes).collect();
    assert_ne!(
        pa, pc,
        "identical packet streams under different seeds -- the seed does not reach the engine RNG"
    );

    // Only on success: a failing run is exactly the one where a human wants
    // the tapes still on disk to diff.
    let _ = std::fs::remove_dir_all(&dir);
}
