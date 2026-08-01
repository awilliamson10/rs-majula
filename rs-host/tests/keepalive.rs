//! Positive control, contrasting with `no_response_force_logout.rs`'s
//! `no_response_within_100_ticks_force_logs_out_the_bot`: a `NoTimeout`
//! packet sent through `host_send` on a well-under-100-tick cadence keeps
//! `ActivePlayer::last_response` fresh, so the no-response force-logout in
//! `phases/logout.rs` never fires and every tick keeps producing outbound
//! bytes.
//!
//! `host_send` pushes onto the SAME wire `ActivePlayer::decode` reads a real
//! client's bytes from, so the packet must be genuinely ISAAC-encoded, not
//! a raw opcode byte. This mirrors `rl-env/tests/spike_protocol_tap.rs`'s
//! `inbound_packets_drive_the_real_client_handlers`: a from-scratch
//! `Isaac::new(&[0; 4])` decode-side mirror stays in lockstep with the
//! engine's `ClientHandle::isaac_decode` (both seeded `[0; 4]` by
//! `spawn_player_tapped`'s `create_io(IsaacPair::new(&[0; 4], &[0; 4]))`),
//! as long as it advances exactly once per packet the engine actually
//! decodes -- true here since we send and let the engine consume one
//! packet at a time.
//!
//! # Why the outbound-emptiness check alone is not enough
//!
//! `ActivePlayer::decode` (`rs-engine/src/active_player.rs:1908-1942`) sets
//! its `received` flag -- which is what refreshes `last_response` -- the
//! moment `inbox.try_recv()` succeeds, BEFORE `read()` decrypts or
//! recognizes any opcode. So ANY inbound bytes, correctly ISAAC-encoded or
//! not, satisfy the "outbound stream never goes silent" check below. That
//! check alone would not catch a Task-4 author who gets the encoding wrong:
//! the engine's real `isaac_decode` still consumes one keystream value for
//! every byte it POPS AS AN OPCODE ATTEMPT regardless of whether it turns
//! out to be recognized, so a single wrongly-encoded byte permanently
//! desyncs the engine's stream from anything encoded correctly afterward --
//! while this test's own outbound-emptiness check would stay green the
//! whole time, since `last_response` doesn't care whether the byte decoded
//! to anything real.
//!
//! So after the keepalive loop, this also sends one real `MoveGameClick`
//! (same wire shape as `spike_protocol_tap.rs`'s
//! `inbound_packets_drive_the_real_client_handlers`) using the SAME
//! continued `isaac_decode` mirror, and checks the bot actually moved via
//! `host_player_x`/`host_player_z` -- the documented test-only use those
//! accessors exist for. That only passes if the mirror is still in lockstep
//! after four keepalives, i.e. if the keepalive encoding was genuinely
//! correct and not just "some bytes showed up."
//!
//! In its own file/process for the same "one `host_new` per process"
//! reason documented in `no_response_force_logout.rs`.

use rs_crypto::isaac::Isaac;

/// rev-274 `ClientProt::NoTimeout` (`rs-protocol/src/network/game/client_prot.rs`,
/// the `#[cfg(rev = "274")]` block: `NoTimeout = 120`). Opcodes are
/// rev-scoped -- never hardcode across revisions. `Fixed` frame, zero-length
/// payload (`#[client_prot(Fixed, ClientEvent)]` with no explicit size), so
/// the whole wire packet is this one ISAAC-encoded byte, no length prefix,
/// no payload.
const NO_TIMEOUT: u8 = 120;

/// rev-274 `ClientProt::MoveGameClick` (`rs-protocol/.../client_prot.rs:523`,
/// same constant `spike_protocol_tap.rs` uses). `VarByte` frame: wire =
/// `[opcode+isaac][len:u8][payload]`. Payload per `MoveGameClick::decode`:
/// `ctrl:g1, x:g2(BE), z:g2(BE)`.
const MOVE_GAMECLICK: u8 = 207;

#[test]
#[ignore = "boots the full world; run on the desktop"]
fn no_timeout_keepalive_prevents_the_force_logout() {
    let h = rs_host::host_new(5151);
    assert!(!h.is_null());

    let mut isaac_decode = Isaac::new(&[0; 4]);

    let mut empty = 0u32;
    for tick in 0..150u32 {
        // Cadence well under the 100-tick force-logout window: send on tick
        // 0 and every 40 ticks after (0, 40, 80, 120).
        if tick % 40 == 0 {
            let byte = (NO_TIMEOUT as u32).wrapping_add(isaac_decode.next_int()) as u8;
            let packet = [byte];
            rs_host::host_send(h, packet.as_ptr(), packet.len());
        }
        if rs_host::host_step(h) == 0 {
            empty += 1;
        }
    }

    // Weak on its own (see the module doc) -- kept as a coarse sanity check,
    // not the discriminator. The real discriminator is the MoveGameClick
    // check below.
    assert_eq!(
        empty, 0,
        "{empty} of 150 ticks produced no outbound bytes even with a \
         keepalive sent every 40 ticks -- inbound bytes are not reaching \
         `ActivePlayer::decode` at all (a stronger failure than a bad \
         encoding: see the MoveGameClick check below for that case). \
         Contrast with no_response_force_logout.rs's negative control, \
         which force-logs-out the same bot with zero inbound traffic."
    );

    // The real discriminator: prove the ISAAC mirror is still in lockstep
    // after four keepalives by sending a real, recognizable action and
    // observing its effect through the test-only ground-truth accessors.
    let start_x = rs_host::host_player_x(h);
    let start_z = rs_host::host_player_z(h);
    assert!(start_x >= 0 && start_z >= 0, "player missing after keepalive loop");

    let dest_x = (start_x + 1) as u16;
    let dest_z = start_z as u16;
    let payload = [
        0u8, // ctrl
        (dest_x >> 8) as u8,
        (dest_x & 0xFF) as u8,
        (dest_z >> 8) as u8,
        (dest_z & 0xFF) as u8,
    ];
    let mut buf = Vec::with_capacity(2 + payload.len());
    buf.push((MOVE_GAMECLICK as u32).wrapping_add(isaac_decode.next_int()) as u8);
    buf.push(payload.len() as u8);
    buf.extend_from_slice(&payload);
    rs_host::host_send(h, buf.as_ptr(), buf.len());

    for _ in 0..10 {
        rs_host::host_step(h);
    }

    let end_x = rs_host::host_player_x(h);
    let end_z = rs_host::host_player_z(h);
    // The DESTINATION, not merely "somewhere else". A single-tile walk on open
    // Tutorial Island ground is deterministic and 10 ticks is ample for it, so
    // pinning the exact tile is free and strictly stronger: `assert_ne!` would
    // also be satisfied by the bot wandering off for some unrelated reason, or
    // by a garbled packet that happened to decode to a different movement.
    assert_eq!(
        (end_x, end_z),
        (start_x + 1, start_z),
        "the bot did not walk to the tile the MoveGameClick asked for \
         ({}, {}) after four keepalives -- it is at ({end_x}, {end_z}). \
         Unchanged means the ISAAC mirror desynced somewhere in the keepalive \
         loop (a wrongly-encoded keepalive would pass the outbound-emptiness \
         check above while silently breaking every packet sent after it); \
         some OTHER tile means the packet decoded to something else entirely",
        start_x + 1,
        start_z
    );

    rs_host::host_free(h);
}
