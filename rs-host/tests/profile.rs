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

    // -- ★★ a magic-valid, CRC-valid, but STRUCTURALLY TRUNCATED blob --------
    //
    // Regression for the Critical finding on this task's first round:
    // `rs_engine::player_save::load_binary`'s two gates before this point
    // (a magic number, a CRC32 over whatever bytes are actually present) are
    // both satisfiable by a blob whose own `varp_count`/inventory `size`
    // fields claim more data than the buffer holds -- and everything past
    // those gates reads through vendored `rs_io::Packet` getters that do
    // raw, unchecked pointer arithmetic with NO bounds check against the
    // buffer's real length. Before `load_binary` grew its own bounds checks,
    // this was a segfault through `host_load_profile`, not a catchable
    // error -- worse than the panic the ★★ MUST NOT PANIC constraint already
    // forbids. A plain truncation of a real blob would fail the CRC gate and
    // prove nothing (see `player_save.rs`'s `sign_crc` doc comment for the
    // same point); these are honestly CRC-signed over their own, shorter
    // content, so they reach the same code that real save data does.
    //
    // `SAV_MAGIC`, `SAV_VERSION` and `STAT_COUNT` are private in
    // `rs_engine::player_save` (rightly -- they are that module's format
    // details, not part of the C ABI), so they are duplicated here as
    // literals, same reasoning as `truth_accessors.rs`'s
    // `HOST_FIELD_UNKNOWN` duplication: a test that imported them would keep
    // passing if the format itself changed underneath it.
    const SAV_MAGIC: u16 = 0x2004;
    const SAV_VERSION: u16 = 7;
    const STAT_COUNT: usize = 21;

    fn write_valid_prefix(sav: &mut rs_io::Packet) {
        sav.p2(SAV_MAGIC);
        sav.p2(SAV_VERSION);
        sav.p2(0); // x
        sav.p2(0); // z
        sav.p1(0); // y
        for _ in 0..7 {
            sav.p1(0); // body
        }
        for _ in 0..5 {
            sav.p1(0); // colors
        }
        sav.p1(0); // gender
        sav.p2(0); // runenergy
        sav.p4(0); // playtime (version >= 2)
        for _ in 0..STAT_COUNT {
            sav.p4(0); // xp
            sav.p1(1); // level
        }
    }

    fn sign_crc(sav: &mut rs_io::Packet) {
        let checksum = rs_io::crc::getcrc(&sav.data, 0, sav.pos);
        sav.p4(checksum);
        sav.data.truncate(sav.pos);
    }

    // Truncated inside the varp section: claims 100 varp entries (400
    // bytes), supplies 8.
    let mut varp_truncated = rs_io::Packet::new(256);
    write_valid_prefix(&mut varp_truncated);
    varp_truncated.p2(100);
    for _ in 0..2 {
        varp_truncated.p4(0);
    }
    sign_crc(&mut varp_truncated);
    assert_eq!(
        rs_host::host_load_profile(h, varp_truncated.data.as_ptr(), varp_truncated.data.len()),
        0,
        "a varp-section-truncated blob must be rejected, not crash the process"
    );

    // Truncated inside an inventory: claims a 50-slot inventory, supplies 2
    // slots' worth of item bytes.
    let mut inv_truncated = rs_io::Packet::new(256);
    write_valid_prefix(&mut inv_truncated);
    inv_truncated.p2(0); // varp_count: none
    inv_truncated.p1(1); // inv_count: one inventory
    inv_truncated.p2(0); // inv_type
    inv_truncated.p2(50); // size claims 50 slots...
    for _ in 0..2 {
        inv_truncated.p2(1); // id_raw (non-zero)
        inv_truncated.p1(1); // count_byte
    }
    sign_crc(&mut inv_truncated);
    assert_eq!(
        rs_host::host_load_profile(h, inv_truncated.data.as_ptr(), inv_truncated.data.len()),
        0,
        "an inventory-truncated blob must be rejected, not crash the process"
    );
}
