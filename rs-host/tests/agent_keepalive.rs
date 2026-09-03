//! ★★ THE ~100-TICK SILENT LOGOUT, FOR AN AGENT THAT IS NOT THE FIRST.
//!
//! `host_step`'s doc comment states the deadline: a player whose
//! `last_response` is `TIMEOUT_NO_RESPONSE = 100` ticks stale is
//! force-logged-out, and `last_response` moves only when
//! `ActivePlayer::decode` sees inbound bytes. `host_add_agent` (Task 4) gave
//! every agent a reachable inbox, but until `host_agent_send` existed there
//! was no way to put a byte in any inbox but the first agent's -- so a second
//! agent could not answer, and died around tick 100.
//!
//! **The symptom is not an error.** `host_agent_out_len` just returns 0 from
//! then on, forever, and the client on the other end renders a stale frame. A
//! run shorter than ~100 ticks (Task 5's own 60-tick oracle, for one) never
//! sees it; Task 6's 190-tick tutorial does.
//!
//! # The control is in this same process, on purpose
//!
//! `host_new` may be called once per process (`rs-pathfinder`'s
//! process-global `COLLISION_FLAGS`; see `second_agent.rs`), so the negative
//! control cannot be a second `#[test]`. It is a second EXTRA AGENT instead:
//! `alive` is sent keepalives, `doomed` is sent nothing, and both are read at
//! the same ticks. That pairing is what makes this discriminating rather than
//! merely green -- it fails an implementation that keeps everyone alive
//! (`host_agent_send` ignoring its `pid` and broadcasting) exactly as loudly
//! as one that keeps nobody alive.
//!
//! ★ The encoding matters as much as the delivery, for the reason
//! `keepalive.rs`'s module doc sets out at length: `decode` refreshes
//! `last_response` the moment `try_recv` succeeds, before it decrypts
//! anything, so ANY bytes pass a liveness check. Here the keepalives are
//! genuinely ISAAC-encoded against a from-scratch `Isaac::new(&[0; 4])`
//! mirror -- `spawn_player_tapped` seeds every agent's pair with `[0; 4]`, so
//! each agent needs its OWN mirror, advanced once per packet sent to it.

use rs_crypto::isaac::Isaac;

/// Lumbridge, and one tile east of it. Deliberately off Tutorial Island so
/// neither extra agent is standing on the first one -- see `second_agent.rs`.
const ALIVE_SPAWN: (u16, u8, u16) = (3222, 0, 3218);
const DOOMED_SPAWN: (u16, u8, u16) = (3223, 0, 3218);

/// rev-274 `ClientProt::NoTimeout` (`rs-protocol/src/network/game/client_prot.rs`,
/// the `#[cfg(rev = "274")]` block). `Fixed` frame, zero-length payload, so
/// the whole wire packet is this one ISAAC-encoded byte. Opcodes are
/// rev-scoped -- never hardcode across revisions.
const NO_TIMEOUT: u8 = 120;

/// Comfortably inside the 100-tick window, and more than one keepalive lands
/// before the deadline, so a single dropped packet would not be the thing
/// that decides this test.
const KEEPALIVE_EVERY: u32 = 40;

/// Past the deadline by half again, so a force-logout at ~100 has plenty of
/// ticks left to show up as a silent feed.
const TICKS: u32 = 150;

fn out_len(h: *mut std::ffi::c_void, pid: u16) -> usize {
    rs_host::host_agent_out_len(h, pid)
}

#[test]
#[ignore = "boots the full world; run on the desktop"]
fn host_agent_send_keeps_one_agent_alive_past_the_100_tick_deadline() {
    let h = rs_host::host_new(5252);
    assert!(!h.is_null());

    let (ax, alevel, az) = ALIVE_SPAWN;
    let alive = rs_host::host_add_agent(h, ax, alevel, az) as u16;
    let (dx, dlevel, dz) = DOOMED_SPAWN;
    let doomed = rs_host::host_add_agent(h, dx, dlevel, dz) as u16;
    assert_ne!(alive, 0, "host_add_agent reported a pid collision");
    assert_ne!(doomed, 0, "host_add_agent reported a pid collision");
    assert_ne!(alive, doomed, "the two extra agents share a pid");

    let mut isaac = Isaac::new(&[0; 4]);

    let mut alive_silent_after_deadline = 0u32;
    let mut doomed_silent_after_deadline = 0u32;

    for tick in 0..TICKS {
        if tick % KEEPALIVE_EVERY == 0 {
            let byte = (NO_TIMEOUT as u32).wrapping_add(isaac.next_int()) as u8;
            let packet = [byte];
            let dropped = rs_host::host_agent_send(h, alive, packet.as_ptr(), packet.len());
            assert_eq!(dropped, 0, "host_agent_send dropped the keepalive at tick {tick}");
        }

        rs_host::host_step(h);

        // ★ Only past the deadline. Before it both agents are alive and both
        // feeds are busy, so counting there would measure nothing.
        if tick > 110 {
            if out_len(h, alive) == 0 {
                alive_silent_after_deadline += 1;
            }
            if out_len(h, doomed) == 0 {
                doomed_silent_after_deadline += 1;
            }
        }
    }

    // The negative control FIRST: if this one is alive too, the positive
    // result below proves nothing about `host_agent_send` -- it would mean
    // the force-logout is not firing in this build at all, or that the send
    // ignored its `pid`.
    assert_eq!(
        doomed_silent_after_deadline,
        TICKS - 111,
        "the agent that was sent NOTHING is still receiving packets after \
         tick 110 ({} of {} ticks were silent). Either the 100-tick \
         force-logout is not firing here -- in which case this test's \
         positive half proves nothing -- or `host_agent_send` ignored its \
         `pid` and kept it alive too.",
        doomed_silent_after_deadline,
        TICKS - 111
    );

    assert_eq!(
        alive_silent_after_deadline, 0,
        "the agent that WAS sent keepalives went silent for {} of the {} \
         ticks after 110 -- it was force-logged-out anyway, which means the \
         bytes never reached `ActivePlayer::decode` for this pid.",
        alive_silent_after_deadline,
        TICKS - 111
    );

    // A null pointer or an unknown pid is a reported drop, never a send to
    // someone else -- crossing two agents' inbound streams would desync both
    // ISAAC pairs with nothing to report it.
    assert_eq!(rs_host::host_agent_send(h, alive, std::ptr::null(), 1), 1);
    assert_eq!(rs_host::host_agent_send(h, alive + 1000, [0u8].as_ptr(), 1), 1);
    // Zero length is a no-op, not a drop.
    assert_eq!(rs_host::host_agent_send(h, alive, [0u8].as_ptr(), 0), 0);

    rs_host::host_free(h);
}
