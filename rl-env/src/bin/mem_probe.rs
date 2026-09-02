//! MEASUREMENT (throwaway): what is the 370 MB `host_new` costs actually made of?
//!
//! An RL rollout wants many envs on one box, and today each one is its own
//! process because `rs-pathfinder` keeps `COLLISION_FLAGS` process-global. The
//! cost of that isolation depends entirely on a split nothing has measured:
//!
//!   - `shared_cache()` returns a MEMOIZED `&'static CacheStore`, built at most
//!     once per process. N engines in ONE process would share it; N processes
//!     each pay for their own copy.
//!   - everything `Engine::new` allocates on top is per-engine either way.
//!
//! If the cache dominates, making one process hold several engines is worth
//! real money and the `COLLISION_FLAGS` work is justified. If `Engine::new`
//! dominates, process isolation is nearly free and the current design stands.
//!
//! Also prices `boot_arena` (skips static world NPCs) against `boot_seeded`.
//!
//! Usage: cargo run -p rl-env --release --bin mem_probe [-- arena]

// ★ THE HYPOTHESIS THIS TESTS. `pack_all` reads ~588 MB of content to produce
// a cache whose live byte blobs total 8 MB. glibc's malloc keeps freed arenas
// rather than returning them to the OS, so most of the resident 316 MB may be
// PACKING GARBAGE the allocator is sitting on, not live data. If so it is
// reclaimable with one call, and 64 envs stop paying for it 64 times.
unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}

fn hwm_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("VmHWM:") {
            return v.trim().trim_end_matches(" kB").trim().parse().unwrap_or(0);
        }
    }
    0
}

fn rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            return v.trim().trim_end_matches(" kB").trim().parse().unwrap_or(0);
        }
    }
    0
}

fn at(label: &str, prev: u64) -> u64 {
    let now = rss_kb();
    println!(
        "RSS {{\"at\":\"{}\",\"rss_mb\":{:.1},\"delta_mb\":{:.1},\"peak_mb\":{:.1}}}",
        label,
        now as f64 / 1024.0,
        (now as f64 - prev as f64) / 1024.0,
        hwm_kb() as f64 / 1024.0
    );
    now
}

fn main() {
    let arena = std::env::args().any(|a| a == "arena");
    let mut prev = at("start", 0);

    // The memoized process-lifetime cache, on its own. `boot_inner` calls this
    // first thing, so isolating it here measures exactly the part that N
    // engines in one process would NOT duplicate.
    let (store, _scripts) = rl_env::shared_cache();
    prev = at("shared_cache", prev);

    // ★ WHICH PART OF THE CACHE IS IT? These three fields are plain byte blobs
    // (`Arc<[u8]>` / `Box<[u8]>`), so they are the part that could be mmapped
    // read-only and shared by every process, or trimmed to the regions an env
    // actually visits. Everything else is decoded type providers, which cannot
    // be either without a serialization format.
    let jags: usize = store.jags.values().map(|b| b.len()).sum();
    let maps: usize = store.mapsquares.values().map(|b| b.len()).sum();
    let ondemand: usize = store.ondemand.iter().flat_map(|v| v.iter()).map(|b| b.len()).sum();
    let mb = |b: usize| b as f64 / (1024.0 * 1024.0);
    println!(
        "BLOBS {{\"jags_mb\":{:.1},\"mapsquares_mb\":{:.1},\"mapsquare_count\":{},\"ondemand_mb\":{:.1},\"blob_total_mb\":{:.1}}}",
        mb(jags), mb(maps), store.mapsquares.len(), mb(ondemand), mb(jags + maps + ondemand)
    );

    // Hand every arena back to the OS and re-read. Nothing live is freed by
    // this — it only releases what the allocator was already holding empty.
    unsafe { malloc_trim(0) };
    prev = at("after_malloc_trim", prev);

    let _env = if arena {
        rl_env::EnvHarness::boot_arena_seeded(4242)
    } else {
        rl_env::EnvHarness::boot_seeded(4242)
    };
    prev = at(if arena { "boot_arena" } else { "boot_seeded" }, prev);
    unsafe { malloc_trim(0) };
    let _ = at("boot_then_trim", prev);
}
