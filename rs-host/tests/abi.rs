use std::ffi::CString;

#[test]
#[ignore = "boots the full world; run on the desktop"]
fn the_c_abi_exposes_a_live_engine_cache_and_packet_stream() {
    let h = rs_host::host_new(4242);
    assert!(!h.is_null());

    // A fresh account, or the whole G1 benchmark is meaningless.
    let tutorial = CString::new("tutorial").unwrap();
    assert_eq!(rs_host::host_varp(h, tutorial.as_ptr()), 0);

    // The cache is reachable and non-trivial.
    let cfg = CString::new("config").unwrap();
    assert!(rs_host::host_cache_len(h, cfg.as_ptr()) > 100_000);
    let crc = CString::new("crc").unwrap();
    assert!(rs_host::host_cache_len(h, crc.as_ptr()) > 0);

    // On-demand archive 0 (models) file 1 exists and is gzip.
    let len = rs_host::host_ondemand_len(h, 0, 1);
    assert!(len > 0, "model 1 missing");
    let bytes = unsafe { std::slice::from_raw_parts(rs_host::host_ondemand_ptr(h, 0, 1), len) };
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "on-demand blobs must be gzip wire bytes");

    // First tick: `host_out_ptr`/`host_out_len` are what Task 4 actually
    // reads -- exercise them directly (not just `host_step`'s return), and
    // check the documented shape: `host_out_len` must agree with the
    // `host_step` return, and the buffer must begin with the RAW, non-ISAAC
    // login response `[2, staffmodlevel, mouseTracked]` (Task 1's verified
    // fact) -- this tick's buffer holds accept_login's whole buffered login
    // burst plus tick 0's own cycle output, so that prefix must be first.
    let step0 = rs_host::host_step(h);
    let out_len = rs_host::host_out_len(h);
    assert_eq!(
        out_len, step0 as usize,
        "host_out_len disagrees with host_step's own return for the same tick"
    );
    assert!(
        out_len >= 3,
        "tick 0's outbound buffer is only {out_len} bytes -- too short to \
         hold the login response prefix"
    );
    let out0 = unsafe { std::slice::from_raw_parts(rs_host::host_out_ptr(h), out_len) };
    // `[opcode, staffmodlevel, mouseTracked]`. `mouseTracked` is a hardcoded
    // `1` (`Engine::accept_login`, `rs-engine/src/engine.rs:2580`); opcode 2
    // is `LoginResponse::SuccessNormal`. `staffmodlevel` is
    // `active.player.staff_mod_level.min(2)`, and a freshly-constructed
    // `Player` defaults that field to `StaffModLevel::Developer` under
    // `debug_assertions` (`rs-engine/rs-entity/src/player.rs:295`) or
    // `StaffModLevel::Normal` in a release build (`:297`) -- so its exact
    // value is build-profile-dependent, not something this test (which runs
    // as a plain `cargo test`, i.e. always `debug_assertions`) should pin to
    // a single number Task 4 might see differently under `--release`. Assert
    // the two invariant bytes and the documented `.min(2)` clamp instead.
    assert_eq!(out0[0], 2, "expected LoginResponse::SuccessNormal (2)");
    assert!(out0[1] <= 2, "staffmodlevel must be clamped to <= 2, got {}", out0[1]);
    assert_eq!(out0[2], 1, "mouseTracked is always 1");

    // Remaining ticks: every tick produces outbound bytes. PlayerInfo is
    // sent every tick, so a silent tap detach would show up as zeros here.
    let mut empty = if step0 == 0 { 1 } else { 0 };
    for _ in 0..9 {
        if rs_host::host_step(h) == 0 {
            empty += 1;
        }
    }
    assert_eq!(empty, 0, "{empty} of 10 ticks produced no outbound bytes");

    // `host_send`'s drop signal, exercised in BOTH directions against a real
    // inbox. A dropped message is a permanent ISAAC desync that nothing else
    // in the fused loop can observe (see `host_send`'s doc comment), so the
    // return value is the only warning the consumer will ever get -- and a
    // return value nothing checks is worth nothing.
    //
    // The inbox is a bounded `channel(128)` that the engine drains inside
    // `cycle()`. Filling it without stepping is therefore the exact
    // saturation an agent acting every tick can produce, not a contrived one.
    // Done last: these bytes are junk and would desync the very mirror the
    // other tests depend on.
    const INBOX_CAPACITY: usize = 128; // `host_new`'s `channel::<Vec<u8>>(128)`
    let junk = [0u8; 1];
    for i in 0..INBOX_CAPACITY {
        assert_eq!(
            rs_host::host_send(h, junk.as_ptr(), junk.len()),
            0,
            "send {i} of {INBOX_CAPACITY} reported a drop while the inbox \
             still had room"
        );
    }
    assert_eq!(
        rs_host::host_send(h, junk.as_ptr(), junk.len()),
        1,
        "the {}th send into a {INBOX_CAPACITY}-slot inbox was silently \
         accepted -- a full channel must report the drop, or the consumer \
         desyncs the engine's ISAAC stream with no way to find out",
        INBOX_CAPACITY + 1
    );
    // A null pointer with a non-zero length delivers nothing either, and is
    // reported the same way rather than passing for success.
    assert_eq!(rs_host::host_send(h, std::ptr::null(), 4), 1);
    // Nothing to deliver is not a drop.
    assert_eq!(rs_host::host_send(h, junk.as_ptr(), 0), 0);

    rs_host::host_free(h);
}
