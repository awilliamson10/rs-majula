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

    assert_eq!(
        empty, 0,
        "{empty} of 150 ticks produced no outbound bytes even with a \
         NoTimeout keepalive sent every 40 ticks -- either the keepalive \
         packet is not reaching the real client handler, or the ISAAC \
         mirror fell out of lockstep. Contrast with \
         no_response_force_logout.rs's negative control, which force-logs- \
         out the same bot with zero inbound traffic."
    );

    rs_host::host_free(h);
}
