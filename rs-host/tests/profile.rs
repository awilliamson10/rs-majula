//! Profile save/restore across the C ABI.
//!
//! ★★ ONE ENGINE PER PROCESS, THEREFORE ONE `#[test]` FN IN THIS BINARY.
//! `host_new` aborts on a second call and `rs-pathfinder` keeps COLLISION_FLAGS
//! process-global, so a second `#[test]` here would abort the binary rather
//! than run. `rs-host/tests/truth_accessors.rs` says the same thing and holds
//! exactly one test for the same reason. Every assertion goes in this one fn.
//!
//! ★ The point is not that the bytes round-trip — `player_save.rs` covers that
//! — but that they round-trip THROUGH THE POINTER API, including the
//! too-small-buffer and null paths, without panicking. A panic in an
//! `extern "C"` fn aborts the process rather than raising.

#[test]
fn profile_round_trips_through_the_abi_and_never_panics() {
    // ★ No `--spawn` equivalent: `host_new` is Tutorial Island, which is where
    // Phase 1 episodes live. `host_new_at` would be the mainland.
    let h = rs_host::host_new(4242);
    assert!(!h.is_null());
    for _ in 0..20 {
        rs_host::host_step(h);
    }

    // -- the happy path --------------------------------------------------------
    let mut buf = vec![0u8; 64 * 1024];
    let n = rs_host::host_save_profile(h, buf.as_mut_ptr(), buf.len());
    assert!(n > 0, "host_save_profile returned {n}");
    assert_eq!(
        rs_host::host_load_profile(h, buf.as_ptr(), n as usize),
        1,
        "host_load_profile rejected the blob host_save_profile just wrote"
    );

    // -- ★ the too-small buffer reports the size it needs and writes nothing ---
    let mut tiny = vec![0u8; 4];
    let need = rs_host::host_save_profile(h, tiny.as_mut_ptr(), tiny.len());
    assert!(need < -1, "expected a negative required size, got {need}");
    assert_eq!(-need, n, "the reported size disagrees with the real blob length");
    assert_eq!(tiny, vec![0u8; 4], "it wrote into a buffer it had declared too small");

    // -- ★★ the null paths return sentinels rather than aborting ---------------
    assert_eq!(rs_host::host_save_profile(h, std::ptr::null_mut(), 0), -1);
    assert_eq!(rs_host::host_load_profile(h, std::ptr::null(), 0), 0);
    assert_eq!(rs_host::host_load_profile(h, buf.as_ptr(), 0), 0);
}
