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
        let mut ticks = Vec::with_capacity(count);
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
pub fn digest(t: &Tape) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    };
    for b in t.seed.to_le_bytes() {
        eat(b);
    }
    for tick in &t.ticks {
        for b in tick.tick.to_le_bytes() {
            eat(b);
        }
        for &b in &tick.bytes {
            eat(b);
        }
    }
    h
}
