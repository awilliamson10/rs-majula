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
/// | 4  | `target_player` — the pid this npc is fighting, or -1            |
/// | 5  | `hunt_mode` — the hunt config id, or -1                          |
/// | 6  | tile x                                                          |
/// | 7  | tile z                                                          |
/// | 8  | npc TYPE id (`uid.id()`)                                        |
/// | 9  | `active` — 0 for an npc that is dead and awaiting respawn        |
///
/// Anything else returns [`HOST_FIELD_UNKNOWN`].
///
/// # ★ FIELDS 6, 7 AND 8 ARE NOT HIDDEN STATE — do not use them as probe targets
///
/// The client IS told an npc's type id and position; that is how it renders the
/// model on the right tile. They are here as JOIN KEYS — field 8 is what lets a
/// label row be matched to the packed npc config, and it is the independent path
/// `tests/truth_accessors.rs` checks the hitpoints index against. A probe scored
/// on them would report near-perfect accuracy for reading out a column it was
/// handed. Fields 1-5 and 9 are the hidden ones. (There is no `level` field
/// because [`host_npc_count`] filters to the player's own level, so it would be a
/// constant.)
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
/// Fields 3, 4 and 5 report `None` as -1. That is why the sentinel is
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

#[unsafe(no_mangle)]
pub extern "C" fn host_free(h: *mut c_void) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h as *mut Host)) }
    }
}
