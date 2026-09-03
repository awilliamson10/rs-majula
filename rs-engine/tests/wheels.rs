//! ★★ The wheels are the LABEL CHANNEL, not deleted state. Suppression must
//! change what reaches the SOCKET and nothing else — every engine-side effect
//! (modal_tutorial tracking, IfClose triggers, %tutorial gating) still runs.
use rs_engine::wheels::{self, HintLabel};
use std::sync::Mutex;

/// ★★ SERIALIZED, DELIBERATELY. All three tests below touch the SAME
/// pid's entry in the SAME shared map (`wheels.rs`'s doc comment: the
/// store is process-wide, keyed by pid, not per-test). Rust's default
/// test harness runs the tests in this file concurrently across threads,
/// so without this lock
/// `suppression_defaults_to_off`'s unsynchronized read of `SUPPRESS` can
/// interleave with `the_toggle_round_trips`'s `set_suppressed(true)` before
/// it flips back to `false` — a spurious failure that would look like a bug
/// in the suppression flag rather than in the test's unstated assumption
/// that it runs alone. `the_toggle_round_trips` restores `false` before
/// releasing the lock, so serialized order does not matter, only exclusion.
/// A poisoned lock (one test panicking mid-hold) must not fail every other
/// test in the file, hence `unwrap_or_else(|e| e.into_inner())` — the same
/// pattern `wheels.rs` uses for its own statics, and for the same reason.
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn suppression_defaults_to_off() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!wheels::suppressed(), "default must be off or make tape stops printing 2208");
}

#[test]
fn a_recorded_hint_is_readable_and_overwritten_by_the_next() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    wheels::forget(1);
    wheels::record_hint(1, HintLabel::Npc(42));
    assert_eq!(wheels::hint(1), HintLabel::Npc(42));
    wheels::record_hint(1, HintLabel::Tile { x: 3222, z: 3218 });
    assert_eq!(wheels::hint(1), HintLabel::Tile { x: 3222, z: 3218 });
    wheels::record_hint(1, HintLabel::None);
    assert_eq!(wheels::hint(1), HintLabel::None);
}

#[test]
fn the_toggle_round_trips() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    wheels::set_suppressed(true);
    assert!(wheels::suppressed());
    wheels::set_suppressed(false);
    assert!(!wheels::suppressed());
}
