//! ★ NO ENGINE IS BOOTED. The accessors read the process-global label store, so
//! the host pointer is ignored and these tests are pure. That is also why a
//! NULL host must be safe: nothing dereferences it.
use rs_engine::wheels::{self, HintLabel};
use std::sync::Mutex;

/// ★★ SERIALIZED, DELIBERATELY, matching `rs-engine/tests/wheels.rs`'s own
/// `TEST_LOCK` (commit `03bc30b3`) rather than solving the identical problem a
/// second way. These three tests reach the SAME process-global label store
/// `rs-engine/tests/wheels.rs` does, just through the C ABI wrappers instead
/// of `rs_engine::wheels` directly, and Rust's default test harness runs a
/// file's tests concurrently across threads. Held for the FIRST line of every
/// test. `unwrap_or_else(|e| e.into_inner())` so a panicking test cannot
/// poison the lock for the rest of the file.
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn each_hint_kind_reports_its_own_payload() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let h = std::ptr::null_mut();

    wheels::record_hint(HintLabel::Npc(42));
    assert_eq!(rs_host::host_hint_kind(h), 1);
    assert_eq!(rs_host::host_hint_a(h), 42);
    assert_eq!(rs_host::host_hint_b(h), -1, "an npc hint has no second coordinate");

    wheels::record_hint(HintLabel::Tile { x: 3222, z: 3218 });
    assert_eq!(rs_host::host_hint_kind(h), 2);
    assert_eq!(rs_host::host_hint_a(h), 3222);
    assert_eq!(rs_host::host_hint_b(h), 3218);

    wheels::record_hint(HintLabel::None);
    assert_eq!(rs_host::host_hint_kind(h), 0);
    assert_eq!(rs_host::host_hint_a(h), -1);
}

#[test]
fn the_flash_tab_reports_minus_one_when_there_is_none() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let h = std::ptr::null_mut();
    wheels::record_flash_tab(None);
    assert_eq!(rs_host::host_flash_tab(h), -1);
    wheels::record_flash_tab(Some(3));
    assert_eq!(rs_host::host_flash_tab(h), 3);
    // ★ Restored before releasing the lock, matching `the_toggle_round_trips`'s
    // own discipline in `rs-engine/tests/wheels.rs` -- serialized order does
    // not matter here, only leaving the global at a value the NEXT lock-holder
    // is not surprised by.
    wheels::record_flash_tab(None);
}

/// ★ NO HANDLE ARGUMENT, DELIBERATELY -- see `host_wheels_suppress`'s own doc
/// comment in `rs-host/src/lib.rs`. `host_new`/`host_new_at` run the login
/// trigger (and so the island's first wheel) synchronously, before either
/// returns a handle at all, so a switch gated on one could never be flipped
/// in time to catch it. This test calls it with no host booted at all, which
/// is the whole point of the signature: it must be reachable before a `Host`
/// exists.
#[test]
fn the_suppression_switch_is_reachable_over_the_abi_before_any_host_exists() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    rs_host::host_wheels_suppress(1);
    assert!(wheels::suppressed());
    rs_host::host_wheels_suppress(0);
    assert!(!wheels::suppressed());
}
