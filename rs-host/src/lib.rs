//! C-ABI surface for the fused sim process.
//!
//! Bun `dlopen`s this and drives the engine directly — no sockets, no login
//! handshake, no cache over HTTP. `bun:ffi` reads the returned pointers with
//! `toArrayBuffer`, which is a VIEW, not a copy — see [`host_out_ptr`]'s doc
//! comment for the buffer's validity window before holding onto one.
//!
//! ★ ONE ENGINE PER PROCESS: `rs-pathfinder` holds process-global collision
//! state. `host_new` panics if called a second time in this process.
//!
//! ★ SINGLE-THREADED CALLER ONLY. `Host` is neither `Send` nor `Sync`, and
//! nothing here enforces that at the C boundary: every entry point fabricates
//! a `&mut Host` out of a raw pointer (see [`host_ref`]) with no borrow
//! tracking whatsoever, so two threads holding the same handle would hold two
//! simultaneous `&mut` to the same `Host` -- instant UB, not a data race the
//! engine would merely be slow about. `host_step` also mutates engine state
//! that `rs-pathfinder` keeps process-global. Drive one handle from one
//! thread; the fused loop's consumer (Bun) is single-threaded by construction.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};

use rl_env::tape::TUTORIAL_SPAWN;
use rl_env::EnvHarness;
use rs_grid::CoordGrid;
use rs_pack::cache::CacheStore;
use tokio::sync::mpsc::UnboundedReceiver;

/// Guards against a second `host_new` in this process. `rs-pathfinder` holds
/// process-global `COLLISION_FLAGS`; a second `Engine` would silently
/// corrupt both instead of failing loud, so this asserts instead.
static BOOTED: AtomicBool = AtomicBool::new(false);

pub struct Host {
    env: EnvHarness,
    pid: u16,
    /// Outbound: the player's `handle.outbox` was replaced with our sender.
    rx: UnboundedReceiver<Vec<u8>>,
    /// Inbound: the player's `handle.inbox` is a Receiver, so we must own the
    /// paired Sender to push client packets in.
    tx_in: tokio::sync::mpsc::Sender<Vec<u8>>,
    out: Vec<u8>,
    /// The process's single, `'static` cache instance -- see [`rl_env::cache`]'s
    /// doc comment. Deliberately NOT a `Box<CacheStore>` this `Host` owns:
    /// an owned copy would (a) dangle every cache pointer Bun holds the
    /// moment `host_free` runs, since nothing in the C ABI's contract says
    /// cache pointers die with the `Host`, and (b) be a SEPARATE pack from
    /// the one `EnvHarness::boot_seeded` -> `shared_cache()` already built
    /// for the engine itself, so the client would decode against different
    /// cache bytes than the engine runs on. `rl_env::cache()` returns the
    /// same memoized, process-lifetime `&'static CacheStore` the engine
    /// uses, packed at most once per process.
    cache: &'static CacheStore,
    empty: Vec<u8>,
}

/// Boots the full world and spawns one fresh Tutorial Island player.
///
/// # Panics
/// If called more than once in this process (see [`BOOTED`]).
#[unsafe(no_mangle)]
pub extern "C" fn host_new(seed: u64) -> *mut c_void {
    assert!(
        !BOOTED.swap(true, Ordering::SeqCst),
        "host_new called twice -- ONE ENGINE PER PROCESS (rs-pathfinder's \
         COLLISION_FLAGS is process-global)"
    );

    // ★ AMENDED after Task 1: use `boot_seeded` and `spawn_player_tapped`.
    // A tap installed AFTER `spawn_player` misses `accept_login`'s entire
    // client bootstrap (rebuild_normal, varps, stats, pid) because
    // `create_io`'s receiver is dropped when `spawn_player` returns — and no
    // later tick re-sends RebuildNormal, so the client can never build a
    // scene. `spawn_player_tapped` retains the receiver and passes the handle
    // INTO `accept_login`, so the stream is the true socket feed from login
    // onward. It also removes the ISAAC re-seat: `create_io` already seeds
    // `[0; 4]`, so a from-scratch mirror is in lockstep by construction.
    let mut env = EnvHarness::boot_seeded(seed);

    let (x, level, z) = TUTORIAL_SPAWN;
    let (pid, rx) = env.engine.spawn_player_tapped("agent", CoordGrid::new(x, level, z));

    let (tx_in, rx_in) = tokio::sync::mpsc::channel::<Vec<u8>>(128);
    {
        let p = env.engine.get_player_mut(pid).expect("spawned player");
        // Replace the inbox so we hold the sending end.
        p.handle.inbox = rx_in;
    }

    // `boot_seeded` (above) already populated the process's memoized cache
    // cell via `shared_cache()` -- this just borrows that same instance, not
    // a fresh pack. See `Host::cache`'s doc comment.
    let cache = rl_env::cache();

    Box::into_raw(Box::new(Host {
        env,
        pid,
        rx,
        tx_in,
        out: Vec::new(),
        cache,
        empty: Vec::new(),
    })) as *mut c_void
}

#[inline]
fn host_ref<'a>(h: *mut c_void) -> &'a mut Host {
    assert!(!h.is_null(), "null host handle");
    unsafe { &mut *(h as *mut Host) }
}

/// Advances one tick and buffers that tick's outbound bytes. Returns their length.
///
/// # The caller MUST send `NoTimeout` at least every 100 ticks
///
/// This bot is `bot: false` (a genuine `accept_login`, not the `bot: true`
/// fabricated-handle path -- deliberately NOT switched to avoid changing
/// engine behaviour beyond the timeout), so it is on the *unguarded* side of
/// `phases/logout.rs`'s no-response force-logout: `arena_mode` is `false`
/// (this is a full-world boot), and `!bot && !arena_mode` is exactly the
/// branch that force-logs-out a player once `clock - last_response >= 100`
/// ticks (`TIMEOUT_NO_RESPONSE`). `last_response` only advances when
/// `ActivePlayer::decode` sees at least one inbound message that tick. A
/// real client answers this with the `NoTimeout` housekeeping packet, and
/// the fused-loop consumer must do the same: push an encoded `NoTimeout`
/// (rev-274 opcode 120, `Fixed` frame, zero-length payload -- see
/// `rs-protocol/src/network/game/client_prot.rs`'s `rev = "274"` block)
/// through [`host_send`] at least once every 100 ticks, or this player is
/// force-logged-out around tick 100 and every subsequent `host_step` call
/// returns 0 forever (the outbox sender is dropped with the removed
/// player). See `rs-host/tests/no_response_force_logout.rs`'s
/// `no_response_within_100_ticks_force_logs_out_the_bot` and
/// `rs-host/tests/keepalive.rs`'s
/// `no_timeout_keepalive_prevents_the_force_logout` for a discriminating
/// demonstration of both directions (two separate test binaries/processes,
/// each calling `host_new` exactly once -- see [`BOOTED`]).
///
/// ★ `rl_env::tape::record_tutorial_tape` (Task 1) has this exact same
/// latent limit -- any tape recorded past ~100 ticks needs the same
/// treatment. Not fixed here; flagged for whoever extends that recorder.
#[unsafe(no_mangle)]
pub extern "C" fn host_step(h: *mut c_void) -> u32 {
    let host = host_ref(h);
    host.env.engine.cycle();
    host.out.clear();
    while let Ok(buf) = host.rx.try_recv() {
        host.out.extend_from_slice(&buf);
    }
    host.out.len() as u32
}

/// Zero-copy view of the last [`host_step`] call's outbound bytes.
///
/// # Validity
/// The returned pointer is valid ONLY until the next [`host_step`] or
/// [`host_free`] call on this handle. `host_step` does `out.clear()` then
/// `extend_from_slice`, which reallocates whenever a tick's payload exceeds
/// the buffer's current capacity -- guaranteed to happen at least once,
/// since tick 0's login burst dwarfs the ~tens-of-bytes steady-state tick.
/// `bun:ffi`'s `toArrayBuffer` hands back a VIEW over this pointer, not a
/// copy, so a caller that holds one across a `host_step` call is reading
/// freed or reused memory. Copy the bytes out before stepping again.
#[unsafe(no_mangle)]
pub extern "C" fn host_out_ptr(h: *mut c_void) -> *const u8 {
    host_ref(h).out.as_ptr()
}

/// Length in bytes of the buffer [`host_out_ptr`] points at. Same validity
/// window as `host_out_ptr` -- read together, before the next `host_step`.
#[unsafe(no_mangle)]
pub extern "C" fn host_out_len(h: *mut c_void) -> usize {
    host_ref(h).out.len()
}

/// Pushes inbound client bytes into the player's inbox; the engine's own
/// `decode` dispatches them through the real client-message handlers.
///
/// # Returns
/// `0` if the bytes were queued, `1` if they were DROPPED (`len > 0` with a
/// null `ptr`, or the 128-slot inbox was full).
///
/// # ★ A DROP IS A PERMANENT, SILENT ISAAC DESYNC -- never ignore this
///
/// `ActivePlayer::read` subtracts one `isaac_decode.next_int()` from every
/// byte it pops as an opcode ATTEMPT, before it knows whether the opcode is
/// real (`rs-engine/src/active_player.rs:1963-1975`). The client's encoder
/// advances its own keystream for every byte it WROTE. So a message that
/// never arrives leaves the two streams offset by however many bytes were
/// lost, for the rest of the run: every packet after it decodes to noise.
///
/// Nothing downstream notices. `ActivePlayer::decode` sets `received = true`
/// the moment `inbox.try_recv()` succeeds -- so the surviving traffic still
/// refreshes `last_response`, the player stays logged in, `host_step` keeps
/// returning bytes, and the frame keeps rendering. The failure surfaces only
/// as an agent whose actions stop having effects.
///
/// Not reachable while the consumer sends a handful of bytes per tick against
/// a 128-slot channel the engine drains every `cycle()`. It becomes reachable
/// the moment an agent acts every tick, which is the whole point of this
/// crate -- hence a return value rather than a comment.
#[unsafe(no_mangle)]
pub extern "C" fn host_send(h: *mut c_void, ptr: *const u8, len: usize) -> u32 {
    if len == 0 {
        return 0;
    }
    if ptr.is_null() {
        return 1;
    }
    let host = host_ref(h);
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    // The engine's own `ActivePlayer::decode` drains this inbox during the
    // input phase and dispatches through the real client-message handlers.
    match host.tx_in.try_send(bytes) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn cache_slice<'a>(host: &'a Host, name: &str) -> &'a [u8] {
    if name == "crc" {
        return &host.cache.crctable_bytes;
    }
    host.cache
        .jags
        .get(name)
        .map(|a| &a[..])
        .unwrap_or(&host.empty)
}

#[unsafe(no_mangle)]
pub extern "C" fn host_cache_ptr(h: *mut c_void, name: *const c_char) -> *const u8 {
    let host = host_ref(h);
    if name.is_null() {
        return host.empty.as_ptr();
    }
    let n = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
    cache_slice(host, n).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn host_cache_len(h: *mut c_void, name: *const c_char) -> usize {
    let host = host_ref(h);
    if name.is_null() {
        return 0;
    }
    let n = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
    cache_slice(host, n).len()
}

fn ondemand_slice<'a>(host: &'a Host, archive: u32, file: u32) -> &'a [u8] {
    host.cache
        .ondemand
        .get(archive as usize)
        .and_then(|a| a.get(file as usize))
        .map(|b| &b[..])
        .unwrap_or(&host.empty)
}

#[unsafe(no_mangle)]
pub extern "C" fn host_ondemand_ptr(h: *mut c_void, archive: u32, file: u32) -> *const u8 {
    ondemand_slice(host_ref(h), archive, file).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn host_ondemand_len(h: *mut c_void, archive: u32, file: u32) -> usize {
    ondemand_slice(host_ref(h), archive, file).len()
}

/// Returned by [`host_varp`] for a name the cache does not know, a null or
/// non-UTF-8 name, or a player that is gone.
///
/// ★ NOT -1. A varp can legitimately hold -1 (`action_delay` and friends are
/// signed and content writes negative values), so -1 as "unknown" would make a
/// typo and a real reading indistinguishable to the caller. `i32::MIN` is not
/// a value the var table ever holds.
pub const HOST_VARP_UNKNOWN: i32 = i32::MIN;

/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process — the runtime cannot
/// unwind across a C frame, so there is no JS-visible error, just a dead host.
/// `EnvHarness::player_varp` panics on an unknown name, so routing this through
/// `try_player_varp` is the whole difference. Returns [`HOST_VARP_UNKNOWN`] for
/// a null pointer, a non-UTF-8 name, an unknown name, or a departed player.
///
/// # ★ THIS IS ENGINE TRUTH — the reward/checkpoint channel
/// `%tutorial` is the benchmark's primary metric. It is read by the SCORER and
/// by parity tests, and it must never reach `ClientState`: the agent's
/// observation is a pure function of the client's decoded state, and an
/// engine-truth field in it would break faithfulness silently. The TypeScript
/// side enforces that by exporting this accessor from `truth.ts` only.
#[unsafe(no_mangle)]
pub extern "C" fn host_varp(h: *mut c_void, name: *const c_char) -> i32 {
    let host = host_ref(h);
    if name.is_null() {
        return HOST_VARP_UNKNOWN;
    }
    // ★ NOT `.unwrap_or("")`: an empty name would then be looked up as if the
    // caller had asked for it, and the cache's answer for "" is the same
    // `None` — correct by accident today, wrong the moment "" is a debugname.
    let Ok(n) = (unsafe { CStr::from_ptr(name) }).to_str() else {
        return HOST_VARP_UNKNOWN;
    };
    host.env
        .try_player_varp(host.pid, n)
        .unwrap_or(HOST_VARP_UNKNOWN)
}

/// Engine-truth position. ★ For the Task-4 state-parity test ONLY. The agent
/// must never read this — its observation is the client's decoded state, and
/// routing engine truth into the agent path would break faithfulness.
#[unsafe(no_mangle)]
pub extern "C" fn host_player_x(h: *mut c_void) -> i32 {
    let host = host_ref(h);
    match host.env.engine.get_player(host.pid) {
        Some(p) => p.player.pathing.coord.x() as i32,
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn host_player_z(h: *mut c_void) -> i32 {
    let host = host_ref(h);
    match host.env.engine.get_player(host.pid) {
        Some(p) => p.player.pathing.coord.z() as i32,
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn host_free(h: *mut c_void) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h as *mut Host)) }
    }
}
