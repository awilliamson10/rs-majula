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

#[test]
#[ignore = "boots the full world; run on the desktop"]
fn no_response_within_100_ticks_force_logs_out_the_bot() {
    let h = rs_host::host_new(5150);
    assert!(!h.is_null());

    let mut last = u32::MAX;
    for _ in 0..150 {
        last = rs_host::host_step(h);
    }

    assert_eq!(
        last, 0,
        "expected the bot to have been force-logged-out (silent outbox) by \
         tick 150 with zero inbound traffic the whole run; got a nonzero \
         tick instead -- either the no-response timeout no longer fires on \
         this path, or something is unexpectedly feeding it traffic"
    );

    rs_host::host_free(h);
}
