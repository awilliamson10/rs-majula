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

    // Every tick produces outbound bytes: PlayerInfo is sent every tick, so a
    // silent tap detach would show up as zeros here.
    let mut empty = 0;
    for _ in 0..10 {
        if rs_host::host_step(h) == 0 {
            empty += 1;
        }
    }
    assert_eq!(empty, 0, "{empty} of 10 ticks produced no outbound bytes");

    rs_host::host_free(h);
}
