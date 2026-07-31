//! C-ABI surface for the fused sim process.
//!
//! Bun `dlopen`s this and drives the engine directly — no sockets, no login
//! handshake, no cache over HTTP. `bun:ffi` reads the returned pointers with
//! `toArrayBuffer`, so buffers are handed over without copying on this side.
//!
//! ★ ONE ENGINE PER PROCESS: `rs-pathfinder` holds process-global collision
//! state. Never call `host_new` twice in one process.

use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};

use rl_env::tape::TUTORIAL_SPAWN;
use rl_env::EnvHarness;
use rs_grid::CoordGrid;
use rs_pack::cache::CacheStore;
use tokio::sync::mpsc::UnboundedReceiver;

/// `rs_pack::CONTENT_DIR` / `PACK_DIR` are relative paths (`content/274`,
/// `content/274/pack`) intended to be resolved against the workspace root.
/// Whoever loads this cdylib (a `cargo test` binary, or Bun via `dlopen`)
/// may be running with an arbitrary process cwd, so -- mirroring
/// `rl_env::workspace_root`'s fix for the same problem -- resolve against
/// `CARGO_MANIFEST_DIR` (baked in at compile time as `majula/rs-host`)
/// instead of trusting the runtime cwd.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs-host has a parent workspace directory")
        .to_path_buf()
}

pub struct Host {
    env: EnvHarness,
    pid: u16,
    /// Outbound: the player's `handle.outbox` was replaced with our sender.
    rx: UnboundedReceiver<Vec<u8>>,
    /// Inbound: the player's `handle.inbox` is a Receiver, so we must own the
    /// paired Sender to push client packets in.
    tx_in: tokio::sync::mpsc::Sender<Vec<u8>>,
    out: Vec<u8>,
    cache: Box<CacheStore>,
    empty: Vec<u8>,
}

/// Boots the full world and spawns one fresh Tutorial Island player.
#[unsafe(no_mangle)]
pub extern "C" fn host_new(seed: u64) -> *mut c_void {
    let root = workspace_root();
    let content_dir = root.join(rs_pack::CONTENT_DIR);
    let pack_dir = root.join(rs_pack::PACK_DIR);
    let (cache, _scripts) = rs_pack::pack_all(
        &content_dir,
        &pack_dir,
        true,
        true,
    )
    .expect("pack cache");

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

#[unsafe(no_mangle)]
pub extern "C" fn host_out_ptr(h: *mut c_void) -> *const u8 {
    host_ref(h).out.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn host_out_len(h: *mut c_void) -> usize {
    host_ref(h).out.len()
}

/// Pushes inbound client bytes into the player's inbox; the engine's own
/// `decode` dispatches them through the real client-message handlers.
#[unsafe(no_mangle)]
pub extern "C" fn host_send(h: *mut c_void, ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let host = host_ref(h);
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    // The engine's own `ActivePlayer::decode` drains this inbox during the
    // input phase and dispatches through the real client-message handlers.
    let _ = host.tx_in.try_send(bytes);
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
    let n = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
    cache_slice(host, n).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn host_cache_len(h: *mut c_void, name: *const c_char) -> usize {
    let host = host_ref(h);
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

#[unsafe(no_mangle)]
pub extern "C" fn host_varp(h: *mut c_void, name: *const c_char) -> i32 {
    let host = host_ref(h);
    let n = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
    host.env.player_varp(host.pid, n)
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
