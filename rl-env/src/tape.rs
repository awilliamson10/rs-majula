//! A recorded outbound packet stream: the oracle tape.
//!
//! One entry per engine tick holding that tick's concatenated outbound bytes,
//! exactly as the client's socket would have received them. Replaying the same
//! seed must reproduce the same tape byte-for-byte — that property is the
//! project's determinism gate, and `digest` is what the cross-process check
//! compares.

const MAGIC: &[u8; 8] = b"CSTAPE01";

pub struct TapeTick {
    pub tick: u32,
    pub bytes: Vec<u8>,
}

pub struct Tape {
    pub seed: u64,
    pub ticks: Vec<TapeTick>,
}

pub struct TapeWriter {
    seed: u64,
    ticks: Vec<TapeTick>,
}

impl TapeWriter {
    pub fn new(seed: u64) -> Self {
        Self { seed, ticks: Vec::new() }
    }

    /// Records one tick. `packets` are this tick's outbound buffers in send
    /// order; they are concatenated because that is exactly how they would
    /// have arrived on a socket.
    pub fn record_tick(&mut self, tick: u32, packets: &[Vec<u8>]) {
        let mut bytes = Vec::new();
        for p in packets {
            bytes.extend_from_slice(p);
        }
        self.ticks.push(TapeTick { tick, bytes });
    }

    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.seed.to_le_bytes());
        out.extend_from_slice(&(self.ticks.len() as u32).to_le_bytes());
        for t in &self.ticks {
            out.extend_from_slice(&t.tick.to_le_bytes());
            out.extend_from_slice(&(t.bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&t.bytes);
        }
        out
    }
}

pub struct TapeReader;

impl TapeReader {
    pub fn parse(bytes: &[u8]) -> Result<Tape, String> {
        if bytes.len() < 20 || &bytes[..8] != MAGIC {
            return Err("bad magic or truncated header".into());
        }
        let seed = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        let mut pos = 20;
        // `count` comes straight off disk and is not yet corroborated by
        // anything -- the loop below only discovers a lie about it once it
        // walks off the end. `Vec::with_capacity` runs FIRST, so a corrupt
        // header claiming 4 billion ticks aborts the process on the
        // allocation before any of those truncation checks can return `Err`.
        // Every tick costs at least its 8-byte header, so the file's own
        // length is a hard ceiling on how many there can really be; clamping
        // to it costs nothing on a well-formed tape (which reallocs at most
        // once if the clamp bites) and turns an OOM abort into the `Err` the
        // caller already handles.
        let mut ticks = Vec::with_capacity(count.min(bytes.len() / 8));
        for _ in 0..count {
            if pos + 8 > bytes.len() {
                return Err("truncated tick header".into());
            }
            let tick = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
            let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            pos += 8;
            if pos + len > bytes.len() {
                return Err("truncated tick payload".into());
            }
            ticks.push(TapeTick { tick, bytes: bytes[pos..pos + len].to_vec() });
            pos += len;
        }
        Ok(Tape { seed, ticks })
    }
}

/// FNV-1a over the whole stream. Used by the cross-process determinism gate.
///
/// Hashes `ticks.len()` and each tick's `bytes.len()` in addition to the tick
/// number and payload bytes -- without those length fields, the byte stream
/// this feeds the hasher is just the tick-number bytes and payload bytes
/// concatenated with no delimiter, so a single tick numbered 0 with payload
/// `[1, 0, 0, 0]` serializes to the identical 8-byte stream (`[0,0,0,0,
/// 1,0,0,0]`) as two empty ticks numbered 0 and 1 -- a real, checked
/// collision (see `tests/tape_record.rs`'s
/// `digest_is_sensitive_to_tick_structure_not_just_concatenated_bytes`), not
/// a hypothetical one.
pub fn digest(t: &Tape) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    };
    for b in t.seed.to_le_bytes() {
        eat(b);
    }
    for b in (t.ticks.len() as u32).to_le_bytes() {
        eat(b);
    }
    for tick in &t.ticks {
        for b in tick.tick.to_le_bytes() {
            eat(b);
        }
        for b in (tick.bytes.len() as u32).to_le_bytes() {
            eat(b);
        }
        for &b in &tick.bytes {
            eat(b);
        }
    }
    h
}

use crate::EnvHarness;
use rs_grid::CoordGrid;

/// Tutorial Island, level 0. Bounds are x 3053..=3156, z 3056..=3136
/// (`content/274/scripts/tutorial/scripts/util.rs2`, which cites the client's
/// own region test). This tile sits inside them.
pub const TUTORIAL_SPAWN: (u16, u8, u16) = (3094, 0, 3107);

/// Boots the FULL world (the agent needs banks, trees and NPCs -- arena mode
/// is wrong here), spawns one fresh player on Tutorial Island via
/// [`rs_engine::Engine::spawn_player_tapped`], and records `ticks` ticks of
/// its outbound stream.
///
/// Returns `(tape_bytes, tutorial_varp_at_spawn)`.
///
/// # Why `spawn_player_tapped` and not `spawn_player` + a post-hoc tap
///
/// `spawn_player`'s fabricated `ClientIO` -- and the receiver for everything
/// `accept_login`'s `on_login()` sends (`RebuildNormal`, `IfClose`,
/// `UpdatePid`, `ResetClientVarCache`, `SyncVarps`, stats, run energy,
/// `ResetAnims`) -- is dropped the moment `spawn_player` returns. A tap
/// installed afterward (the earlier approach here) never sees any of that:
/// nothing later recovers it, since a per-tick `rebuild_normal(false)` only
/// re-sends the map rebuild once the player has moved more than 4 zones from
/// the build area's origin, which a spawn-in-place tap never triggers.
/// `spawn_player_tapped` keeps the receiver alive from before `accept_login`
/// runs, so the tape captures the true login-through-steady-state feed a
/// real client socket would have received. Because of that, the encoder and
/// a decoder mirroring `Isaac::new(&[0; 4])` from scratch are ALREADY in
/// lockstep -- no re-seat is needed (contrast the older, now-removed
/// approach, which had to re-seat both sides after the fact because the tap
/// attached mid-stream).
pub fn record_tutorial_tape(ticks: u32, seed: u64) -> (Vec<u8>, i32) {
    let mut env = EnvHarness::boot_seeded(seed);
    let (x, level, z) = TUTORIAL_SPAWN;
    let (pid, mut rx) =
        env.engine.spawn_player_tapped("tutorial", CoordGrid::new(x, level, z));

    let tutorial_varp = env.player_varp(pid, "tutorial");

    let mut w = TapeWriter::new(seed);
    for tick in 0..ticks {
        env.engine.cycle();
        let mut packets = Vec::new();
        while let Ok(buf) = rx.try_recv() {
            packets.push(buf);
        }
        w.record_tick(tick, &packets);
    }

    (w.finish(), tutorial_varp)
}
