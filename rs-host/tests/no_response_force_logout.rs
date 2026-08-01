//! Negative control for `rs-host`'s `NoTimeout` keep-alive requirement (see
//! `host_step`'s doc comment in `rs-host/src/lib.rs`). `host_new` boots the
//! FULL world, so `arena_mode == false`, which puts this bot on the
//! *unguarded* side of `phases/logout.rs`'s no-response force-logout:
//! `!bot && !arena_mode` force-logs-out a player once `clock -
//! last_response >= 100` (`TIMEOUT_NO_RESPONSE`), and `last_response` only
//! advances when `ActivePlayer::decode` sees inbound traffic. With zero
//! inbound traffic, that bot is removed a couple of ticks after tick 100,
//! which drops its `handle`'s outbox sender -- so every `host_step` call
//! from then on returns 0.
//!
//! In its own file (own test binary / process) rather than alongside
//! `abi.rs`'s test: `host_new` may only be called once per process (see
//! `rs-host`'s `BOOTED` guard, ONE ENGINE PER PROCESS), so this and its
//! companion positive control (`keepalive.rs`'s
//! `no_timeout_keepalive_prevents_the_force_logout`) each need their own
//! process.

/// `TIMEOUT_NO_RESPONSE` (`phases/logout.rs:28`). Not `pub`, so re-declared
/// here rather than imported; kept in sync by citing the source line.
const TIMEOUT_NO_RESPONSE: u32 = 100;

#[test]
#[ignore = "boots the full world; run on the desktop"]
fn no_response_within_100_ticks_force_logs_out_the_bot() {
    let h = rs_host::host_new(5150);
    assert!(!h.is_null());

    // Record the FIRST silent tick, not just the last one -- "silent at
    // tick 150" is equally consistent with a tap that never attached, a
    // failed spawn, or a broken `host_new`. Pinning the first empty tick to
    // a narrow window right after `TIMEOUT_NO_RESPONSE` (with every earlier
    // tick required to be non-empty) is what actually distinguishes "died
    // of the no-response timeout" from any of those other failure modes.
    let mut first_empty: Option<u32> = None;
    for tick in 0..150u32 {
        if rs_host::host_step(h) == 0 {
            first_empty = Some(tick);
            break;
        }
    }

    // The window opens AT `TIMEOUT_NO_RESPONSE`, not a few ticks before it:
    // the check is `clock - last_response >= 100`, so nothing legitimate can
    // go silent earlier, and allowing tick 95 would only excuse a bug that
    // silenced the outbox for a different reason.
    let window_start = TIMEOUT_NO_RESPONSE;
    let window_end = TIMEOUT_NO_RESPONSE + 15;
    match first_empty {
        Some(tick) => assert!(
            (window_start..=window_end).contains(&tick),
            "bot went silent at tick {tick}, expected it in \
             [{window_start}, {window_end}] (around TIMEOUT_NO_RESPONSE = \
             {TIMEOUT_NO_RESPONSE}); a much earlier tick means the tap \
             never attached or the spawn failed, not the no-response \
             timeout this test targets"
        ),
        None => panic!(
            "expected the bot to be force-logged-out (silent outbox) \
             within 150 ticks of zero inbound traffic; every tick produced \
             outbound bytes instead -- the no-response timeout did not fire"
        ),
    }

    rs_host::host_free(h);
}
