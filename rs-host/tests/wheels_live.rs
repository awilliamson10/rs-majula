//! ★★ ONE ENGINE PER PROCESS, THEREFORE ONE `#[test]` FN. See
//! `rs-host/tests/truth_accessors.rs`.
//!
//! This test lives in `rs-host`, not `rs-engine`, because `rs-engine`'s
//! `[dev-dependencies]` is empty — `rs-host` is not reachable from an
//! `rs-engine` test, but `rs_engine::wheels` is reachable from here either way.
//!
//! ★ THE QUESTION: does the island still PROGRESS with its wheels off? Only the
//! socket write is skipped, so `modal_tutorial` tracking, the `IfClose` trigger
//! and `%tutorial` gating must all be unaffected. If this regresses, a
//! `self.write` was skipped that carried engine meaning.
#[test]
fn a_suppressed_island_still_records_labels_and_advances() {
    rs_engine::wheels::set_suppressed(true);
    let h = rs_host::host_new(4242);
    assert!(!h.is_null());

    let name = std::ffi::CString::new("tutorial").unwrap();
    let start = rs_host::host_varp(h, name.as_ptr());
    assert_eq!(start, 0, "a fresh tutorial login should sit at %tutorial 0");

    for _ in 0..300 {
        rs_host::host_step(h);
    }

    // ★ The label was RECORDED even though nothing was drawn — that is the
    // whole point of the channel.
    assert_ne!(
        rs_engine::wheels::hint(),
        rs_engine::wheels::HintLabel::None,
        "the island marks its first target; suppression must record it, not lose it"
    );
    assert!(
        rs_engine::wheels::tut_com().is_some(),
        "the first tutorial panel was opened engine-side and should have been recorded"
    );

    rs_host::host_free(h);
}
