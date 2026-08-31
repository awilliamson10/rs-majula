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
// ★ The enum behind BOTH aggression selectors. `Npc::hunt_target` and
// `Npc::interaction.target` are the same `Option<InteractionTarget>` type, and
// the variant -- not the payload -- is what distinguishes "this npc is engaging
// a player" from "this npc is walking to a log on the ground". See
// [`INTERACTION_KIND_PLAYER`].
use rs_entity::InteractionTarget;
use rs_grid::CoordGrid;
use rs_pack::cache::CacheStore;
// ★ THE ENGINE'S OWN STAT ORDER, not a literal 3. `Stats<6>` is a bare
// `[u16; 6]` with nothing in it naming the slots, so a hardcoded index is a
// silently-wrong reading waiting to happen. See [`host_npc_field`].
use rs_pack::types::NpcStat;
// ★ THE ENGINE'S OWN TELEPORT, not a coordinate assignment. `ScriptPlayer` is
// the trait the content VM dispatches through: `p_teleport` (opcode 2088,
// `rs-vm/src/ops/player.rs:923`) is literally `player.teleport(pop_coord(s)?)`
// on this trait. See [`host_teleport`] for why that matters.
use rs_vm::engine::ScriptPlayer;
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
    /// The nids [`host_npc_count`] last enumerated, sorted ascending.
    ///
    /// ★ A SNAPSHOT, not a live view, and that is what makes slot indices mean
    /// anything: `host_npc_field` takes a SLOT, so the slot->nid mapping has to
    /// hold still for the whole of a caller's read of one label frame. The
    /// alternative — re-deriving the list inside every `host_npc_field` call —
    /// would silently renumber the slots mid-frame if an npc wandered out of
    /// range between two field reads, and the resulting row would be half one
    /// npc and half another with nothing reporting an error.
    npc_slots: Vec<u16>,
}

/// Boots the full world and spawns one fresh player at an ARBITRARY coordinate.
///
/// ★ A SEPARATE ENTRY POINT rather than a new signature on [`host_new`]: every
/// existing test and both `*-once.ts` runners call `host_new(seed)`, and
/// changing that signature would be a wide, mechanical, error-prone edit for no
/// gain. `host_new` is now a thin wrapper at `TUTORIAL_SPAWN`.
///
/// The engine places the player exactly where asked — there is no walkability
/// check and no clamp on this path — so an unwalkable coordinate is the
/// caller's problem, not a silent relocation.
///
/// # ★★ THE COORDINATE IS A LOGIN LOCATION, AND IT ONLY RECENTLY BECAME ONE
///
/// This is the trap that nearly sank G1c Task 1; do not undo it by "simplifying"
/// `spawn_player_tapped` back into a post-login teleport.
///
/// `on_login()` fires the `[login,_]` trigger, and
/// `content/274/scripts/login_logout/login.rs2:81` branches on where the player
/// is STANDING at that moment:
///
/// ```text
/// if (%tutorial < ^tutorial_complete & ~in_tutorial_island(coord) = true) {
///     @start_tutorial;
/// }
/// ~initalltabs;
/// ```
///
/// `@` is a label jump — control never returns — so once `start_tutorial` is
/// taken it nulls every sidebar tab (`tutorial.rs2:31-43`) and `~initalltabs`
/// on the next line, the ONLY thing in the game that grants them, never runs.
///
/// `accept_login` used to take no coordinate: every player was built at
/// `rs-engine/src/active_player.rs:182`'s hardcoded
/// `CoordGrid::new(3094, 0, 3106)  // Tutorial island`, logged in there, and was
/// relocated afterwards. So EVERY headless spawn, anywhere in the world, ended
/// up with no inventory tab. The cost was silent and entirely downstream:
/// `client-host/src/state.ts` derives `inventoryComId` from the granted tab and
/// returns -1 when there is none, so the observation carried an empty backpack
/// and nothing errored. `accept_login` now takes `Option<CoordGrid>` and applies
/// it before `add_player`/`on_login`; `spawn_player_tapped` passes it.
///
/// Two consequences worth knowing:
///
/// * A mainland spawn is a genuinely fresh account — `%tutorial` stays 0.
///   (It used to settle at 1 everywhere: `start_tutorial` opens `player_kit`,
///   `spawn_player_tapped` closes it, and `[if_close,player_kit]` queues
///   `tutorial_designed_character`.)
/// * A Tutorial Island spawn still gets the tutorial, tabs revoked and all —
///   `host_new`'s behaviour is unchanged, which is what the existing suites pin.
///
/// Pinned by `rs-host/tests/mainland_login_grants_tabs.rs` (engine truth, on
/// `player.tabs`), `rs-host/tests/mainland_spawn.rs` (placement) and
/// `python/tests/test_env.py`'s `mainland` tests (the client-visible result).
///
/// # Panics
/// If called more than once in this process (see [`BOOTED`]).
#[unsafe(no_mangle)]
pub extern "C" fn host_new_at(seed: u64, x: u16, level: u8, z: u16) -> *mut c_void {
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
        npc_slots: Vec::new(),
    })) as *mut c_void
}

/// Tutorial Island, for every caller that predates [`host_new_at`].
///
/// ★ Kept byte-identical in behaviour to what it was before the split: same
/// seed, same `TUTORIAL_SPAWN`, same `BOOTED` guard (inherited from
/// `host_new_at`, so calling either one twice still aborts).
#[unsafe(no_mangle)]
pub extern "C" fn host_new(seed: u64) -> *mut c_void {
    let (x, level, z) = TUTORIAL_SPAWN;
    host_new_at(seed, x, level, z)
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

// -- profile save/restore ----------------------------------------------------
//
// ★ THE EPISODE RESET. Phase 1 restores a tutorial step as an episode start.
// ONE ENGINE PER PROCESS makes a reboot cost a whole process (~40s), so this
// is the difference between an eval sweep in minutes and one in hours.

/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process. Every failure path
/// here returns a sentinel instead: `-1` for "could not", a NEGATIVE REQUIRED
/// SIZE for "your buffer is too small", and a positive count for success.
#[unsafe(no_mangle)]
pub extern "C" fn host_save_profile(h: *mut c_void, out: *mut u8, cap: usize) -> i64 {
    let host = host_ref(h);
    let Some(p) = host.env.engine.get_player(host.pid) else {
        return -1;
    };
    let profile = rs_engine::player_save::extract_profile(&p.player, host.cache);
    let blob = rs_engine::player_save::save_binary(&profile, host.cache);
    if out.is_null() {
        return -1;
    }
    if blob.len() > cap {
        // ★ NEGATIVE SIZE, not -1: the caller must be able to tell "too small,
        // allocate N" from "it failed", and a second call is cheap.
        return -(blob.len() as i64);
    }
    unsafe { std::ptr::copy_nonoverlapping(blob.as_ptr(), out, blob.len()) };
    blob.len() as i64
}

/// # ★★ THE SPLIT-WIRE-MESSAGE FIX (Task 2b)
///
/// Task 2 measured this: restoring a profile onto a LIVE session crashed the
/// client on the very next tick. The mechanism, traced end to end:
///
///   * a real teleport goes `ActivePlayer::tele` -> `Pathing::teleport`
///     (`rs-engine/rs-entity/src/pathing.rs:207`), which sets `self.tele =
///     true`. THAT flag is what `PlayerInfoEncoder::teleport`
///     (`rs-engine/src/info.rs:488-509`) reads to decide whether to write the
///     21-bit absolute-reposition block into this tick's info packet — the
///     wire message telling the CLIENT where ITS OWN local player now sits
///     relative to the build-area origin.
///   * `apply_profile` (above this fn calls it) writes `player.pathing.coord`
///     as a bare struct field. It never touches `tele`, so that movement
///     block is never emitted.
///   * Meanwhile `ActivePlayer::rebuild_normal(false)` runs every tick purely
///     off `|zone - origin| > 4` and does NOT care how the coordinate got
///     there, so it fires and sends `RebuildNormal` regardless.
///
/// The result is exactly the failure class [`host_teleport`]'s own doc
/// comment warns about, but WORSE: there the caller at least gets a status
/// back and the world merely goes stale. Here the client is told "the region
/// changed" with no "here is where you are in it" to go with it, and
/// `Client.ts`'s next `roofCheck` (via `cameraLocalTileX/Z`, derived from the
/// STALE `localPlayer.x/z`) indexes outside the newly built map and throws.
///
/// The fix reuses [`host_teleport`]'s own path rather than hand-setting
/// `tele`: after `apply_profile` has written the restored coordinate onto
/// `pathing.coord`, that SAME coordinate is re-driven through
/// `ScriptPlayer::teleport` (`p.teleport`, the trait method
/// `rs-vm/src/ops/player.rs:923`'s `p_teleport` opcode also dispatches
/// through) — the one path already proven, by `host_teleport` and its tests,
/// to set `tele` and re-focus the entity correctly. Teleporting a coordinate
/// to itself is intentional and harmless: `Pathing::teleport` recomputes
/// `last_step_coord`/facing and sets `tele = true` unconditionally on
/// success: it does not skip the work because the destination matches the
/// current position.
///
/// `clear_waypoints()` runs first for the same reason [`host_teleport`] runs
/// it first: a restored player that was mid-walk before the session moved
/// away would otherwise resume those stale waypoints from the new coordinate.
///
/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process — no unwinding across
/// a C frame means no JS-visible error, just a dead host. Nothing added here
/// can panic: `p.player.pathing.coord` was just written by `apply_profile`
/// from a `CoordGrid` that already went through its masking constructor, so
/// re-teleporting to it cannot introduce a value `CoordGrid` has not already
/// accepted, and `Pathing::teleport`'s only fallible branch
/// (`is_zone_allocated`) returns `None` rather than panicking.
///
/// # ★ THE ONE FAILURE MODE THIS CANNOT PAPER OVER
/// If the restored coordinate's zone is not allocated in the collision map,
/// `Pathing::teleport` returns `None` and leaves `tele` false — the exact
/// same silent gap this fix exists to close, just for a coordinate the
/// caller's own save data pointed at. There is no earlier, side-effect-free
/// point to check this from `host_load_profile` (the zone-allocation test is
/// `Pathing::teleport`'s alone, not exposed separately), so this is detected
/// AFTER `apply_profile` has already run rather than avoided beforehand. In
/// that case every OTHER field from the profile (stats, varps, inventories,
/// appearance) is left fully applied — restoring those is unconditional and
/// infallible field-by-field assignment, see `apply_profile` — and only the
/// coordinate/`tele` half is left in the pre-fix broken state. Rather than
/// claim success on a half-synced client, this returns `0`: a caller that
/// only checks the sentinel gets an honest "did not fully restore" instead of
/// a client that resyncs and one that silently does not looking identical.
/// A save coordinate landing in an unallocated zone should not happen for any
/// engine-authored profile — every position a player can stand at IS an
/// allocated zone — so this is expected to be unreachable in practice; it is
/// handled rather than assumed away because an `extern "C"` boundary is
/// exactly the place "should not happen" stops being good enough.
///
/// # ★★ MUST NOT PANIC. Returns 1 on success, 0 on any failure.
#[unsafe(no_mangle)]
pub extern "C" fn host_load_profile(h: *mut c_void, ptr: *const u8, len: usize) -> u32 {
    let host = host_ref(h);
    if ptr.is_null() || len == 0 {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(profile) = rs_engine::player_save::load_binary(bytes) else {
        return 0;
    };
    let Some(p) = host.env.engine.get_player_mut(host.pid) else {
        return 0;
    };
    rs_engine::player_save::apply_profile(&profile, &mut p.player, host.cache);

    // ★★ See the doc comment above: reuse `host_teleport`'s own path so the
    // client gets the movement block a bare coordinate write never sends.
    let target = p.player.pathing.coord;
    p.player.pathing.clear_waypoints();
    p.teleport(target);
    if !p.player.pathing.tele {
        // The restored coordinate's zone is not allocated. Every other field
        // is fully applied; only position/tele is left unsynced. See "THE ONE
        // FAILURE MODE THIS CANNOT PAPER OVER" above.
        return 0;
    }

    1
}
// -- the label channel -----------------------------------------------------------

/// Returned by [`host_npc_field`] for a slot that does not exist or a field id
/// that is not defined.
///
/// ★★ NOT -1, and this is not a style choice. `respawn_at`, `target_player` and
/// `hunt_mode` are all `Option`s that report `None` as -1, so a -1 sentinel would
/// make "this npc has no target" and "there is no such npc" the same value. A
/// label file built on that ambiguity would train a model to predict a number
/// that means two different things, and nothing anywhere would flag it.
/// `i64::MIN` is not a value any field below can hold.
///
/// Same reasoning as [`HOST_VARP_UNKNOWN`], one type wider.
pub const HOST_FIELD_UNKNOWN: i64 = i64::MIN;

/// How far from the player [`host_npc_count`] looks, in tiles (Chebyshev), on
/// the player's own level.
///
/// ★ NOT AN ARBITRARY NUMBER: it is `BuildArea::PREFERRED_VIEW_DISTANCE`
/// (`rs-engine/rs-entity/src/build.rs:160`), the same distance
/// `ActiveBuildArea::get_nearby_npcs` uses to decide which npcs the client is
/// told about at all (`rs-engine/src/build.rs:200-229`). Matching it means the
/// label domain is "the npcs a human at this client could be looking at", plus
/// the ones standing dead in that same square waiting to respawn — which the
/// client is told nothing about and which are the most interesting labels here.
const LABEL_RADIUS: u16 = 15;

/// Enumerates the npcs near the player and returns how many there are.
///
/// ★★ CALL THIS FIRST, EVERY FRAME. It is what BUILDS the slot list
/// [`host_npc_field`] indexes; a caller that reads fields without refreshing
/// reads whatever the previous frame enumerated. The slots are stable from this
/// call until the next one, sorted ascending by nid, so slot `i` is the same npc
/// on two consecutive frames as long as that npc stayed in range.
///
/// # ★ THE DOMAIN INCLUDES NPCs THE CLIENT CANNOT SEE, deliberately
///
/// This scans `npc_list` by coordinate rather than reading the player's
/// `build_area.npcs` (the set the client has actually been told about). The
/// difference is exactly the npcs that have been killed and are counting down to
/// respawn: `Engine::despawn_npc` removes them from the zone and the renderer —
/// so they vanish from the client's view — but leaves them in `npc_list` with
/// `active = false`, their death coordinate, and `respawn_at` ticking down
/// (`rs-engine/src/phases/npc.rs:117-126`). Enumerating only the client-visible
/// set would make field 3 permanently -1, i.e. a label column with no variance,
/// which is indistinguishable from not having the label at all.
///
/// # ★★ THIS IS A LABEL CHANNEL, NOT AN OBSERVATION
///
/// Nothing here may reach `ClientState`. The experiment this feeds asks whether a
/// model trained on the client's information set INFERS state the client was
/// never sent; if any of these values is also reachable from the observation, the
/// model is handed the answer and the resulting number looks like a discovery
/// while measuring a readout. Labels and observations are physically separate:
/// separate accessors here, a separate `HostTruth` method, a separate protocol op
/// and separate files. Same rule as [`host_varp`].
///
/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process — no unwinding across a C
/// frame means no JS-visible error, just a dead host. Returns 0 rather than
/// panicking when the player is gone.
#[unsafe(no_mangle)]
pub extern "C" fn host_npc_count(h: *mut c_void) -> u32 {
    let host = host_ref(h);

    // ★ Taken out of `host` rather than borrowed in place: the loop below holds
    // an immutable borrow of `host.env` for its whole body, and pushing into a
    // field of the same struct through it is not something the borrow checker
    // will grant across a method call chain. `mem::take` also keeps the
    // allocation across frames, so a 50-npc frame does not allocate.
    let mut slots = std::mem::take(&mut host.npc_slots);
    slots.clear();

    let Some(p) = host.env.engine.get_player(host.pid) else {
        host.npc_slots = slots;
        return 0;
    };
    let coord = p.player.pathing.coord;
    let (px, py, pz) = (coord.x(), coord.y(), coord.z());

    for entry in host.env.engine.npc_list.npcs.iter() {
        let Some(active) = entry.as_ref() else { continue };
        let c = active.npc.pathing.coord;
        // Same level only, matching `get_nearby_npcs`, which looks up zones on
        // the observer's own `y` and so can never return an npc a floor away.
        if c.y() != py {
            continue;
        }
        // ★ `abs_diff` on u16, NOT a subtraction: coordinates are unsigned and
        // an npc west of the player would underflow to ~65000 and be silently
        // excluded — or, with the operands the other way round, silently
        // included from anywhere in the world.
        if c.x().abs_diff(px) > LABEL_RADIUS || c.z().abs_diff(pz) > LABEL_RADIUS {
            continue;
        }
        slots.push(active.npc.uid.nid());
    }

    // ★ Sorted, so a slot index is joinable across ticks. `npc_list.npcs` is
    // indexed BY nid so this scan already produces ascending order today; the
    // sort makes that a guarantee of this function rather than an accident of
    // the engine's storage layout, which the pid/nid allocator's forward-only
    // wraparound could change.
    slots.sort_unstable();

    let n = slots.len() as u32;
    host.npc_slots = slots;
    n
}

// -- the aggression selectors ----------------------------------------------------
//
// ★★ WHY FIELD 4 (`target_player`) IS NOT ENOUGH, measured rather than argued.
// G1c's corpus held `target_player >= 0` in 0 of 1,713,343 npc rows — across
// every process, every segment and every tick — while the SAME corpus carried
// 343 player-damage events, 1,355 client-confirmed attack swings, 57 npc kills
// and 53,134 rows with a live `hunt_mode`. The state exists; the field does not
// receive it. `Npc::target_player` is declared at
// `rs-engine/rs-entity/src/npc.rs:37`, initialised to `None` at `:96`, and
// ASSIGNED NOWHERE IN THE WORKSPACE — grep it: the only read is field 4 below.
//
// Field 4 is kept anyway. A column known to be dead costs one i64 per row, and
// its continued flatness beside fields 10-14 is the check that says the new
// selectors are carrying real state rather than mirroring the old one.

/// [`host_npc_field`] fields 10 and 12: no target at all (`Option::None`).
///
/// ★ -1, not [`HOST_FIELD_UNKNOWN`], for the same reason fields 3/4/5 use -1:
/// "this npc is engaging nothing" is a real, common, in-range answer, whereas
/// the sentinel means the QUESTION was malformed. Collapsing the two would put
/// an error and a fact in the same bucket of a label file.
pub const INTERACTION_KIND_NONE: i64 = -1;
/// A ground object (`InteractionTarget::Obj`). Index = the obj TYPE id.
pub const INTERACTION_KIND_OBJ: i64 = 0;
/// A placed location (`InteractionTarget::Loc`). Index = the loc TYPE id.
pub const INTERACTION_KIND_LOC: i64 = 1;
/// Another npc (`InteractionTarget::Npc`). Index = the target's `nid`.
pub const INTERACTION_KIND_NPC: i64 = 2;
/// ★★ A PLAYER (`InteractionTarget::Player`). Index = the target's `pid`, so
/// this is the pair that `target_player` was supposed to be and never was: an
/// npc whose interaction kind is 3 and whose index is the host's own pid is an
/// npc engaging the agent.
pub const INTERACTION_KIND_PLAYER: i64 = 3;

/// The variant of an interaction target, as one of the `INTERACTION_KIND_*`
/// constants.
///
/// ★ Split from [`interaction_index`] rather than packed into one integer
/// (`kind * 100_000 + index`, say) DELIBERATELY. The kind is a five-valued
/// category and the index is a dense id in a different numbering scheme per
/// kind; a packed column would have to be decoded by hand at analysis time, and
/// an analyst who forgot would get a plausible, monotone, entirely meaningless
/// number. Two columns cost one extra i64 per row and cannot be misread.
const fn interaction_kind(t: &Option<InteractionTarget>) -> i64 {
    match t {
        None => INTERACTION_KIND_NONE,
        Some(InteractionTarget::Obj { .. }) => INTERACTION_KIND_OBJ,
        Some(InteractionTarget::Loc { .. }) => INTERACTION_KIND_LOC,
        Some(InteractionTarget::Npc { .. }) => INTERACTION_KIND_NPC,
        Some(InteractionTarget::Player { .. }) => INTERACTION_KIND_PLAYER,
    }
}

/// The payload index of an interaction target, or -1 when there is none.
///
/// ★★ THE NUMBERING SCHEME DEPENDS ON THE KIND and the two must be read
/// together: `Obj`/`Loc` report a CONFIG TYPE id, `Npc` reports an `nid` and
/// `Player` a `pid`. Reading this column without its kind column would join
/// npc 42 onto player 42 onto item 42.
const fn interaction_index(t: &Option<InteractionTarget>) -> i64 {
    match t {
        None => -1,
        Some(InteractionTarget::Obj { id, .. } | InteractionTarget::Loc { id, .. }) => *id as i64,
        Some(InteractionTarget::Npc { nid }) => *nid as i64,
        Some(InteractionTarget::Player { pid }) => *pid as i64,
    }
}

/// One field of one enumerated npc. See [`host_npc_count`] for the slot list.
///
/// `field` is:
///
/// | id | meaning                                                        |
/// |----|----------------------------------------------------------------|
/// | 0  | `nid` — the engine's npc index                                  |
/// | 1  | current hitpoints                                               |
/// | 2  | max (base) hitpoints                                            |
/// | 3  | `respawn_at` — TICKS REMAINING, or -1 when `None`                |
/// | 4  | `target_player` — ★ A DEAD FIELD, kept as a control. See below.  |
/// | 5  | `hunt_mode` — the hunt config id, or -1                          |
/// | 6  | tile x                                                          |
/// | 7  | tile z                                                          |
/// | 8  | npc TYPE id (`uid.id()`)                                        |
/// | 9  | `active` — 0 for an npc that is dead and awaiting respawn        |
/// | 10 | `hunt_target` KIND — an `INTERACTION_KIND_*`. ★ See the trap.    |
/// | 11 | `hunt_target` INDEX — pid/nid/type id per the kind, or -1        |
/// | 12 | `interaction.target` KIND — an `INTERACTION_KIND_*`              |
/// | 13 | `interaction.target` INDEX — pid/nid/type id per the kind, or -1 |
/// | 14 | `interaction.target_op` — an `NpcMode` discriminant, or -1       |
///
/// Anything else returns [`HOST_FIELD_UNKNOWN`].
///
/// # ★★ FIELD 14 IS AN `NpcMode`, NOT A `ServerTriggerType`
///
/// `InteractionState::target_op` is a bare `u8` shared by players and npcs, and
/// the two read it as DIFFERENT ENUMS. For a player it is a
/// `ServerTriggerType`; for an npc, `npc_process_movement_interaction` decodes
/// it with `NpcMode::try_from` (`rs-engine/src/phases/npc.rs:1104-1112`), and
/// everything this accessor can see is an npc. The table is
/// `rs-pack/src/types.rs:436` — `None`=0, `Wander`=1, `Patrol`=2,
/// `PlayerEscape`=3, `PlayerFollow`=4, `PlayerFace`=5, `PlayerFaceClose`=6,
/// `OpPlayer1..5`=7..11, `ApPlayer1..5`=12..16, then the Loc/Obj/Npc op and ap
/// families up to 46 and the `Queue1..20` modes above that.
///
/// ★ IT IS OFTEN SET WITH NO TARGET. `npc_process_movement_interaction` has a
/// failsafe that writes `target_op = default_mode` whenever both the op and the
/// target are `None` (`phases/npc.rs:1107-1109`), so a wandering npc reads 1
/// with kind -1. "Op says 7-16" AND "kind says 3" together are what mean the
/// npc is running a player-directed mode against a real player.
///
/// # ★★ FIELD 10/11 (`hunt_target`) IS A WITHIN-TICK TEMPORARY — expect -1
///
/// Do not read a flat `hunt_target` column as "nothing hunted anything". The
/// engine sets and CONSUMES it inside one cycle: player-type hunts write it in
/// the WORLD phase (`process_npc_hunt_players`, `phases/world.rs:137-161`), and
/// the npc phase that runs immediately after calls `npc_consume_hunt_target`,
/// whose second statement is `active.npc.hunt_target.take()`
/// (`phases/npc.rs:1020`) — the value is moved out and spent on
/// `set_interaction`. `host_npc_field` can only ever run BETWEEN cycles, which
/// is after the take.
///
/// It survives to the boundary only when the take is skipped: the npc went
/// `delayed` or inactive between the two phases (`process_npc` returns before
/// the consume, `phases/npc.rs:139`), or its `hunt_mode` was cleared, or the
/// hunt id is absent from the cache. Those are real states worth a column, but
/// they are the exception. **Fields 12/13 are where a hunt's outcome LANDS** —
/// `npc_consume_hunt_target` hands the very same `InteractionTarget` to
/// `Npc::set_interaction`, which stores it in `interaction.target` where it
/// persists across ticks until the interaction ends.
///
/// # ★ FIELDS 6, 7 AND 8 ARE NOT HIDDEN STATE — do not use them as probe targets
///
/// The client IS told an npc's type id and position; that is how it renders the
/// model on the right tile. They are here as JOIN KEYS — field 8 is what lets a
/// label row be matched to the packed npc config, and it is the independent path
/// `tests/truth_accessors.rs` checks the hitpoints index against. A probe scored
/// on them would report near-perfect accuracy for reading out a column it was
/// handed. Fields 1-5 and 9-14 are the hidden ones. (There is no `level` field
/// because [`host_npc_count`] filters to the player's own level, so it would be a
/// constant.)
///
/// ★ Fields 10-14 are hidden in the same sense as the rest: the client is sent an
/// npc's FACE-ENTITY mask, which does encode a target as `pid + 32768` or a raw
/// `nid` (`rs-entity/src/interaction.rs`'s `set_face_entity`) — but that is a
/// rendering hint the entity is merely LOOKING at, it is only sent while the mask
/// is flagged, and `ClientState` does not decode it today. If it ever does, these
/// fields move out of the hidden set and this comment is the thing that has to
/// change with them.
///
/// # ★★ THE HITPOINTS INDEX IS THE ENGINE'S OWN, NOT A LITERAL
///
/// `Stats<6>` is `[u16; 6]` and names nothing. `NpcStat::Hitpoints` is 3
/// (`rs-pack/src/types.rs:1033-1040`) — the same slot `ActiveNpc::new` seeds from
/// `npc_type.hitpoints` and `ActiveNpc::damage` decrements
/// (`rs-engine/src/active_npc.rs:55,126`). It happens to coincide with
/// `PlayerStat::Hitpoints`, but the two are different six/twenty-one-wide sets
/// and the coincidence is not something to rely on: reading the enum costs
/// nothing and cannot drift.
///
/// # ★ -1 IS A REAL VALUE HERE
/// Fields 3, 4, 5 and 10-14 all report `None` as -1. That is why the sentinel is
/// [`HOST_FIELD_UNKNOWN`] and not -1 — see its doc comment.
///
/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process. Both indexes are
/// checked: the slot against the snapshot, the nid against `get_npc` (an npc can
/// be removed between the count and the read — `EntityLifeTime` values other than
/// `Respawn` are dropped from `npc_list` entirely).
///
/// ★★ LABEL CHANNEL, NOT OBSERVATION — see [`host_npc_count`].
#[unsafe(no_mangle)]
pub extern "C" fn host_npc_field(h: *mut c_void, slot: u32, field: u32) -> i64 {
    let host = host_ref(h);
    let Some(&nid) = host.npc_slots.get(slot as usize) else {
        return HOST_FIELD_UNKNOWN;
    };
    let Some(active) = host.env.engine.get_npc(nid) else {
        return HOST_FIELD_UNKNOWN;
    };
    let npc = &active.npc;
    const HP: usize = NpcStat::Hitpoints as usize;
    match field {
        0 => nid as i64,
        1 => npc.stats.level(HP) as i64,
        2 => npc.stats.base_level(HP) as i64,
        3 => npc.respawn_at.map_or(-1, |v| v as i64),
        4 => npc.target_player.map_or(-1, |v| v as i64),
        5 => npc.hunt_mode.map_or(-1, |v| v as i64),
        6 => npc.pathing.coord.x() as i64,
        7 => npc.pathing.coord.z() as i64,
        8 => npc.uid.id() as i64,
        9 => i64::from(npc.active),
        10 => interaction_kind(&npc.hunt_target),
        11 => interaction_index(&npc.hunt_target),
        12 => interaction_kind(&npc.interaction.target),
        13 => interaction_index(&npc.interaction.target),
        // ★ `target_op` is `Option<u8>`, so the widening is lossless and the -1
        // can never collide with a real mode (they are 0..=66).
        14 => npc.interaction.target_op.map_or(-1, i64::from),
        _ => HOST_FIELD_UNKNOWN,
    }
}

/// The engine's own tick counter.
///
/// # ★★ WHY THIS EXISTS AT ALL: `action_delay` IS NOT A COUNTDOWN
///
/// The attack cooldown needs NO new npc-style accessor — `action_delay` is a
/// plain player varp (id 58 in `content/274/pack/varp.pack`) and [`host_varp`]
/// already reads it. What it needs is this, because the varp holds an ABSOLUTE
/// tick rather than a remaining count: content writes `%action_delay =
/// calc(map_clock + 3)` and tests it with `if (%action_delay > map_clock)`
/// (`content/274/scripts/quests/quest_legends/scripts/jungle_tree.rs2:61-66`).
/// The cooldown is therefore `max(0, action_delay - clock)`, and a label built
/// from the raw varp would be a monotonically rising tick number wearing the name
/// "cooldown" — plausible, well-typed, and nonsense to train on.
///
/// `client-host/src/truth.ts`'s `actionDelay()` is where the subtraction lives.
///
/// ★★ LABEL CHANNEL, NOT OBSERVATION — see [`host_npc_count`].
///
/// # ★★ MUST NOT PANIC
/// A field read on a struct this handle owns; nothing here can fail.
#[unsafe(no_mangle)]
pub extern "C" fn host_clock(h: *mut c_void) -> i64 {
    host_ref(h).env.engine.clock as i64
}

/// [`host_teleport`]: the player arrived at the coordinate asked for.
pub const HOST_TELEPORT_OK: u32 = 0;
/// [`host_teleport`]: there is no player on this handle any more (logged out,
/// force-logged-out — see [`host_step`]'s keepalive note).
pub const HOST_TELEPORT_NO_PLAYER: u32 = 1;
/// [`host_teleport`]: the ENGINE refused. `Pathing::teleport` early-returns
/// `None` when `rsmod::is_zone_allocated` says the destination zone is not in
/// the collision map, and `ActivePlayer::tele` answers that with an "Invalid
/// teleport!" game message and no movement at all
/// (`rs-engine/src/active_player.rs:2510-2521`).
pub const HOST_TELEPORT_REFUSED: u32 = 2;
/// [`host_teleport`]: the coordinate does not fit `CoordGrid`'s packing and
/// would have been SILENTLY MASKED into a different, perfectly valid one.
pub const HOST_TELEPORT_OUT_OF_RANGE: u32 = 3;

/// `CoordGrid` packs x and z into 14 bits each (`rs-grid/src/coord.rs:49`).
const COORD_MAX: u16 = 0x3FFF;
/// ...and the level into 2.
const LEVEL_MAX: u8 = 3;

/// Moves the player to an arbitrary coordinate: the segment boundary.
///
/// # ★★ THE CLIENT DOES NOT NECESSARILY FOLLOW, AND IT DOES NOT COMPLAIN
///
/// `ActivePlayer::rebuild_normal(false)` — the per-tick call at
/// `phases/info.rs:76` that is the only thing which ever sends `REBUILD_NORMAL`
/// during play — early-returns unless `BuildArea::needs_rebuild` is true, and
/// that is `|zone_x - origin_x| > 4 || |zone_z - origin_z| > 4`
/// (`rs-engine/rs-entity/src/build.rs:285-291`). A zone is 8 tiles, so a hop of
/// 32 tiles or less sends NOTHING and the client keeps rendering the region it
/// already had.
///
/// Nothing about that looks broken from the outside. The local player's own
/// movement is encoded as a teleport RELATIVE TO THE BUILD AREA ORIGIN
/// (`info.rs:238-252`), so the client's idea of the player's absolute tile stays
/// correct — it is the world around the player that is stale. `sceneState` stays
/// 2, the frame still composites, every loc and npc in the observation belongs to
/// somewhere the player is not, and no error is raised anywhere.
///
/// So this returns a status and the CALLER confirms the rebuild by watching
/// `client.mapBuildBaseX`/`mapBuildBaseZ`. See `client-host/src/teleport.ts`.
///
/// # ★ WHY `ScriptPlayer::teleport` AND NOT `pathing.set_coord`
///
/// This is the engine's own teleport, reached through the same trait method the
/// content VM's `p_teleport` opcode dispatches to
/// (`rs-vm/src/ops/player.rs:923`). It runs `ActivePlayer::tele`, which:
///
///   * refuses destinations in an unallocated zone rather than moving there
///     (hence [`HOST_TELEPORT_REFUSED`]);
///   * sets `pathing.tele`, which is what makes `PlayerInfo::write_local_player`
///     choose the absolute-reposition encoding instead of a walk step — without
///     it the client would be told the player took one step and would render it
///     drifting a tile at a time while the engine had it somewhere else;
///   * sets `pathing.jump` on a level change, and re-focuses the entity's facing
///     direction.
///
/// A bare coordinate assignment skips every one of those.
///
/// # ★ THE ONE THING ADDED ON TOP: THE QUEUED PATH
///
/// `Pathing::reset` (run at the head of every tick) clears `walk_step`, the
/// direction fields and the tele/jump flags but NOT `waypoint_index`
/// (`rs-engine/rs-entity/src/pathing.rs`), and neither does `tele`. A player
/// teleported mid-walk therefore keeps the waypoints it was walking to and the
/// movement phase resumes them from the new coordinate — i.e. a segment that
/// began with a 200-tile teleport would spend its first tens of ticks walking
/// back toward the previous region. `clear_waypoints` is a plain data reset with
/// no script execution, so it is safe to run outside a tick; the interaction
/// target is deliberately NOT cleared here, because `clear_pending_action` fires
/// `IfClose` triggers, and running content scripts outside `Engine::cycle` is not
/// something this entry point can do safely (see the MUST NOT PANIC note below).
///
/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process — there is no JS-visible
/// error, just a dead host. Nothing on this path can panic: the range check below
/// runs BEFORE `CoordGrid::new`, which would otherwise mask silently
/// (`x & 0x3FFF`, `y & 0x3`, `z & 0x3FFF`), and `is_zone_allocated` only indexes
/// a global map with values that masking has already made in-range.
///
/// # Returns
/// [`HOST_TELEPORT_OK`], [`HOST_TELEPORT_NO_PLAYER`], [`HOST_TELEPORT_REFUSED`]
/// or [`HOST_TELEPORT_OUT_OF_RANGE`]. Success is confirmed by READING THE
/// COORDINATE BACK, not by trusting that `tele` did anything — that is what turns
/// the engine's silent "Invalid teleport!" into a status the caller can act on.
#[unsafe(no_mangle)]
pub extern "C" fn host_teleport(h: *mut c_void, x: u16, level: u8, z: u16) -> u32 {
    // ★ BEFORE `CoordGrid::new`, which masks rather than rejects. `bun:ffi`
    // has already truncated anything above u16/u8 on the way in, so this is
    // the second of two silent narrowings on the same value; the TypeScript
    // side range-checks ahead of the call for the first one.
    if x > COORD_MAX || z > COORD_MAX || level > LEVEL_MAX {
        return HOST_TELEPORT_OUT_OF_RANGE;
    }
    let target = CoordGrid::new(x, level, z);

    let host = host_ref(h);
    let Some(p) = host.env.engine.get_player_mut(host.pid) else {
        return HOST_TELEPORT_NO_PLAYER;
    };

    // See "THE ONE THING ADDED ON TOP" above. Before the teleport, so a path
    // can never be consumed against the new coordinate.
    p.player.pathing.clear_waypoints();
    p.teleport(target);

    // ★ Read it back. `tele` reports failure by sending a chat message, which
    // this side cannot see, so "did it work" is answered by the coordinate.
    if p.player.pathing.coord.packed() == target.packed() {
        HOST_TELEPORT_OK
    } else {
        HOST_TELEPORT_REFUSED
    }
}

// -- the build-area region clamp -------------------------------------------------
//
// ★★ READ THIS BEFORE USING ANY OF THE THREE FUNCTIONS BELOW.
//
// They clamp `BuildArea::mapsquares` — the set `BuildArea::rebuild_normal`
// recomputes on every >4-zone move — to an inclusive rectangle of mapsquares.
// That is exactly what it says and NOT what a caller reaching for it probably
// wants, because on rev 274 that set never reaches the client:
//
//   * `ActivePlayer::rebuild_normal` writes `mapsquares` into the
//     `RebuildNormal` packet only under `#[cfg(rev = "225")]`. The `since_244`
//     arm — the one that compiles here, `.cargo/config.toml` pins `REV = "274"`
//     — sends `RebuildNormal { zone_x, zone_z }` and nothing else
//     (`rs-engine/src/active_player.rs:880-907`).
//   * The only other reader in the workspace is
//     `handlers/rebuild_get_maps.rs:113`, which gates the `DATA_LAND`/`DATA_LOC`
//     reply to a client's `REBUILD_GET_MAPS`. The rev-274 client never sends
//     that message: `vendor/Client-TS/src` contains no `REBUILD_GET_MAPS`,
//     `DATA_LAND` or `DATA_LOC` at all. It loads terrain by asking on-demand
//     archive 3 for the map files of the mapsquares its own 13x13-zone window
//     covers (`Client.ts:6845-6870`), and this process serves that archive
//     wholesale through `host_ondemand_ptr`.
//
// So on this revision the clamp is measurable through
// [`host_mapsquare_count`] and invisible in the frame. It is kept because it is
// the correct lever on revs <= 245.2 and because the count is the cheapest view
// of the build area there is. Anything that needs the CLIENT to stop drawing a
// region has to act on the on-demand map files, not here. The measurement is in
// `.superpowers/sdd/2026-08-05-g2-pixels/task-2-report.md`.

/// [`host_set_region`]: the region was stored.
pub const HOST_REGION_OK: u32 = 0;
/// [`host_set_region`]: there is no player on this handle any more.
pub const HOST_REGION_NO_PLAYER: u32 = 1;
/// [`host_set_region`]: `mx0 > mx1` or `mz0 > mz1`.
///
/// ★ An error rather than an empty region. A caller that transposed its
/// arguments would otherwise get a build area with NO mapsquares in it, which
/// is a perfectly quiet state the engine has no complaint about.
pub const HOST_REGION_INVERTED: u32 = 2;
/// [`host_set_region`]: a bound does not fit one byte.
///
/// ★ The mapsquare key is `(mx << 8) | mz` (`rs-entity/src/build.rs`), so a
/// coordinate of 256 would be MASKED into 0 and clamp the world to the wrong
/// corner of the map with nothing reporting it.
pub const HOST_REGION_OUT_OF_RANGE: u32 = 3;

/// The largest mapsquare coordinate the `(mx << 8) | mz` key can hold.
const MAPSQUARE_MAX: u16 = 0xFF;

/// Restricts the build area's mapsquare set to an inclusive rectangle.
///
/// ★ TAKES EFFECT ON THE NEXT REBUILD, not immediately: `rebuild_normal` is
/// what recomputes the set, and `ActivePlayer::rebuild_normal(false)` early-
/// returns unless the player has moved more than 4 zones from the build area
/// origin. A caller that wants the clamp applied now must teleport far and back
/// — see `rs-host/tests/region_clamp.rs`.
///
/// ★ See the module note above for what this does NOT do on rev 274.
///
/// # ★★ MUST NOT PANIC
/// Every panic in an `extern "C"` fn aborts the process. The bounds are checked
/// before anything is stored, and a departed player is a status rather than an
/// unwrap.
///
/// # Returns
/// [`HOST_REGION_OK`], [`HOST_REGION_NO_PLAYER`], [`HOST_REGION_INVERTED`] or
/// [`HOST_REGION_OUT_OF_RANGE`]. On any error the previous region is left
/// exactly as it was.
#[unsafe(no_mangle)]
pub extern "C" fn host_set_region(h: *mut c_void, mx0: u16, mz0: u16, mx1: u16, mz1: u16) -> u32 {
    // ★ BEFORE the handle is touched, so a bad argument cannot half-apply.
    if mx0 > MAPSQUARE_MAX || mz0 > MAPSQUARE_MAX || mx1 > MAPSQUARE_MAX || mz1 > MAPSQUARE_MAX {
        return HOST_REGION_OUT_OF_RANGE;
    }
    if mx0 > mx1 || mz0 > mz1 {
        return HOST_REGION_INVERTED;
    }
    let host = host_ref(h);
    let Some(p) = host.env.engine.get_player_mut(host.pid) else {
        return HOST_REGION_NO_PLAYER;
    };
    p.player.build_area.region = Some((mx0, mz0, mx1, mz1));
    HOST_REGION_OK
}

/// Restores the unclamped ±6-zone sweep. A no-op when no region was set.
///
/// ★ Same timing as [`host_set_region`]: the set is not recomputed until the
/// next rebuild.
///
/// # ★★ MUST NOT PANIC
/// A departed player is silently nothing to clear.
#[unsafe(no_mangle)]
pub extern "C" fn host_clear_region(h: *mut c_void) {
    let host = host_ref(h);
    if let Some(p) = host.env.engine.get_player_mut(host.pid) {
        p.player.build_area.region = None;
    }
}

/// How many mapsquares the player's build area currently holds.
///
/// ★ A DIAGNOSTIC, and the only external view of `BuildArea::mapsquares` there
/// is. Returns 0 when the player is gone — which is also a legal count for a
/// region that excludes everything in view, so a caller distinguishing the two
/// needs `host_player_x` beside it.
///
/// # ★★ MUST NOT PANIC
/// A field read behind a checked lookup; nothing here can fail.
#[unsafe(no_mangle)]
pub extern "C" fn host_mapsquare_count(h: *mut c_void) -> u32 {
    let host = host_ref(h);
    match host.env.engine.get_player(host.pid) {
        Some(p) => p.player.build_area.mapsquares.len() as u32,
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn host_free(h: *mut c_void) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h as *mut Host)) }
    }
}

#[cfg(test)]
mod tests {
    //! ★★ IN-CRATE, AND THAT IS THE POINT: these run under `cargo test --lib`
    //! and NOT under `cargo test --test <name>`. A gate that only ran the
    //! integration files would skip them entirely.
    //!
    //! ★★ WHY THEY EXIST BESIDE `tests/aggression_fields.rs`. That file boots a
    //! real engine, and a real Lumbridge fight only ever produces two of the five
    //! arms below: `None` and `Player` (measured — 9,415 rows -1 and 277 rows
    //! player, across 400 ticks of an all-attack sampler). `Obj`, `Loc` and `Npc`
    //! targets are perfectly ordinary engine states that no test this project can
    //! afford to run would reach on demand, so the only way to pin their encoding
    //! is to construct them. An encoder is a pure function; it does not need an
    //! engine to be tested, and it does need testing — a transposed `Obj`/`Loc`
    //! arm is invisible in every live run and wrong in every corpus.

    use super::*;
    use rs_pack::types::{LocAngle, LocLayer, LocShape};

    fn obj() -> Option<InteractionTarget> {
        Some(InteractionTarget::Obj {
            coord: CoordGrid::new(3222, 0, 3218),
            id: 1511,
            count: 1,
        })
    }

    fn loc() -> Option<InteractionTarget> {
        Some(InteractionTarget::Loc {
            coord: CoordGrid::new(3222, 0, 3218),
            id: 1276,
            width: 1,
            length: 1,
            shape: LocShape::CentrepieceStraight,
            angle: LocAngle::North,
            layer: LocLayer::Ground,
        })
    }

    #[test]
    fn every_variant_encodes_to_its_own_kind() {
        assert_eq!(interaction_kind(&None), INTERACTION_KIND_NONE);
        assert_eq!(interaction_kind(&obj()), INTERACTION_KIND_OBJ);
        assert_eq!(interaction_kind(&loc()), INTERACTION_KIND_LOC);
        assert_eq!(
            interaction_kind(&Some(InteractionTarget::Npc { nid: 41 })),
            INTERACTION_KIND_NPC
        );
        assert_eq!(
            interaction_kind(&Some(InteractionTarget::Player { pid: 1 })),
            INTERACTION_KIND_PLAYER
        );
    }

    /// ★ THE KINDS MUST BE DISTINCT, and this is not a tautology about five
    /// literals — it is the check that a later edit cannot give two variants the
    /// same code. Two variants sharing a code would collapse "engaging a player"
    /// into "engaging an npc" in every label row, silently.
    #[test]
    fn the_kinds_are_five_distinct_small_integers() {
        let kinds = [
            INTERACTION_KIND_NONE,
            INTERACTION_KIND_OBJ,
            INTERACTION_KIND_LOC,
            INTERACTION_KIND_NPC,
            INTERACTION_KIND_PLAYER,
        ];
        let mut sorted = kinds.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), kinds.len(), "two variants share a kind code");
        // ★ And none of them can be confused with the sentinel or with a
        // coordinate, which is what makes the kind column a cheap check that the
        // selector is pointed at the right field at all.
        assert!(kinds.iter().all(|&k| (-1..=3).contains(&k)));
    }

    /// ★★ THE NUMBERING SCHEME IS PER-KIND, and each arm reads a DIFFERENT struct
    /// field. Distinct values everywhere, so an arm that read `count` instead of
    /// `id`, or `nid` instead of `pid`, cannot pass.
    #[test]
    fn each_variant_reports_its_own_index() {
        assert_eq!(interaction_index(&None), -1);
        assert_eq!(interaction_index(&obj()), 1511);
        assert_eq!(interaction_index(&loc()), 1276);
        assert_eq!(interaction_index(&Some(InteractionTarget::Npc { nid: 41 })), 41);
        assert_eq!(interaction_index(&Some(InteractionTarget::Player { pid: 1 })), 1);
    }

    /// ★ THE PAIR IS ONE `Option` SPLIT ACROSS TWO COLUMNS: absent together or
    /// present together. A player-kind row beside an index of -1 would be
    /// unjoinable; a -1 kind beside a real index would be a target with no
    /// numbering scheme to read it in.
    #[test]
    fn the_kind_and_the_index_agree_about_absence() {
        for t in [
            None,
            obj(),
            loc(),
            Some(InteractionTarget::Npc { nid: 0 }),
            Some(InteractionTarget::Player { pid: 0 }),
        ] {
            assert_eq!(
                interaction_kind(&t) == INTERACTION_KIND_NONE,
                interaction_index(&t) == -1,
                "kind and index disagree for {t:?}"
            );
        }
    }
}
