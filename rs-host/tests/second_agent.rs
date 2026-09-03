//! ★★ ONE ENGINE PER PROCESS, THEREFORE ONE `#[test]` FN. `rs-pathfinder`
//! holds process-global `COLLISION_FLAGS` and `host_new` asserts on a second
//! call, so a second agent has to arrive INSIDE one boot — which is exactly
//! what `host_add_agent` is for. See `rs-host/tests/wheels_live.rs`.
//!
//! ★★ THE FAILURE THIS GUARDS AGAINST IS NOT A CRASH. Two agents draining
//! into one shared out buffer would interleave two independent packet feeds
//! into one byte stream; whichever client read it second would decode the
//! other's frames as its own and keep running, producing valid-looking wrong
//! pixels with nothing reporting an error. So the assertions below are about
//! SEPARATION — distinct pid, distinct buffer, distinct bytes, each with its
//! own login burst at its own front — not about either agent merely being
//! alive.

/// Lumbridge. Deliberately not `TUTORIAL_SPAWN`, so the second agent is
/// somewhere the first is not and its packet feed cannot coincidentally match.
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

/// `accept_login`'s RAW, non-ISAAC login response: `[2, staffmodlevel,
/// mouseTracked]`. Opcode 2 is `LoginResponse::SuccessNormal` and
/// `mouseTracked` is a hardcoded 1; the middle byte is build-profile-dependent
/// and only clamped, for the reasons `rs-host/tests/abi.rs` sets out at length.
fn assert_login_prefix(who: &str, buf: &[u8]) {
    assert!(buf.len() >= 3, "{who}'s tick-0 buffer is only {} bytes", buf.len());
    assert_eq!(buf[0], 2, "expected LoginResponse::SuccessNormal (2) for {who}");
    assert!(buf[1] <= 2, "staffmodlevel must be clamped to <= 2, got {}", buf[1]);
    assert_eq!(buf[2], 1, "mouseTracked is always 1");
}

/// ★ The buffers are only valid until the next `host_step`, so every read of
/// one is scoped to the tick it belongs to and copied out here rather than
/// held. See `host_agent_out_ptr`'s doc comment.
fn out_of(h: *mut std::ffi::c_void, pid: u16) -> Vec<u8> {
    let len = rs_host::host_agent_out_len(h, pid);
    if len == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(rs_host::host_agent_out_ptr(h, pid), len) }.to_vec()
}

#[test]
fn a_second_agent_gets_its_own_pid_and_its_own_feed() {
    let h = rs_host::host_new(4242);
    assert!(!h.is_null());

    // ★ DISCOVERED, NEVER ASSUMED. `next_pid()` is the engine's, not ours, and
    // a hardcoded 1 would turn a pid-allocation change into a test that passes
    // against the wrong player. `host_pid` is the only door onto it.
    let first = rs_host::host_pid(h) as u16;

    let (x, level, z) = LUMBRIDGE;
    let second = rs_host::host_add_agent(h, x, level, z) as u16;
    assert_ne!(second, first, "the second agent must not reuse the first's pid");
    assert_ne!(second, 0, "host_add_agent reported a pid collision");

    // Tick 0 flushes BOTH agents' buffered login bursts. Each must land at the
    // front of its own buffer: if the two feeds were being appended to one
    // `Vec`, one of these two would be empty and the other would hold both
    // logins back to back, which is precisely the corruption a client cannot
    // detect.
    rs_host::host_step(h);
    assert_login_prefix("the first agent", &out_of(h, first));
    assert_login_prefix("the second agent", &out_of(h, second));

    for _ in 0..9 {
        rs_host::host_step(h);
    }

    let a = out_of(h, first);
    let b = out_of(h, second);
    assert!(!a.is_empty(), "first agent got no packets");
    assert!(!b.is_empty(), "second agent got no packets");

    // ★★ SEPARATE BUFFERS, not one shared one. If these ever alias, both feeds
    // are being appended to the same `Vec` and the interleaving is already
    // happening.
    let ptr_a = rs_host::host_agent_out_ptr(h, first);
    let ptr_b = rs_host::host_agent_out_ptr(h, second);
    assert_ne!(ptr_a, ptr_b, "the two agents share one out buffer");

    // Non-vacuity for that pointer check: distinct allocations would still be
    // wrong if one were a copy of the other. Two players 130 tiles apart with
    // different pids cannot receive byte-identical steady-state output.
    assert_ne!(a, b, "the two agents' feeds are byte-identical -- one is a copy of the other");

    // ★ THE SINGLE-AGENT ABI STILL MEANS THE FIRST AGENT. Everything
    // downstream of this crate (`host/src/ffi.ts`'s `step()`, every
    // `*-once.ts` runner) reads `host_out_ptr`/`host_out_len` with no pid at
    // all, and must keep seeing exactly the feed it saw before this task.
    assert_eq!(rs_host::host_out_len(h), a.len());
    assert_eq!(rs_host::host_out_ptr(h), ptr_a);

    // An unknown pid is a zero-length read, not a panic and not another
    // agent's bytes. `host_agent_out_ptr` still returns a readable pointer so
    // the `(ptr, len)` pair is safe to hand to `toArrayBuffer` unconditionally.
    let ghost = second + 1000;
    assert_eq!(rs_host::host_agent_out_len(h, ghost), 0);
    assert!(!rs_host::host_agent_out_ptr(h, ghost).is_null());

    rs_host::host_free(h);
}
