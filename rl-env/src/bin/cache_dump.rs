//! SPIKE (throwaway): dump the in-memory packed cache to disk.
//!
//! `rs_pack::pack_all` returns a `CacheStore` and never writes files, and
//! `rs-server` (which normally serves these archives over HTTP) does not boot
//! here. This lets the Client-TS headless spike load a REAL cache without
//! first needing the napi binding — decoupling the two remaining unknowns.
//!
//! Client-TS asks for `/crc` and then `/<name><crc>` for each archive, so we
//! write `crc` plus one file per jag, named exactly as the client names them.
//!
//! Usage: `cargo run -p rl-env --bin cache_dump -- <out_dir>`

use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let out: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "cache_dump".to_string())
        .into();
    std::fs::create_dir_all(&out)?;

    eprintln!("packing from {} ...", rs_pack::CONTENT_DIR);
    let (store, _scripts) = rs_pack::pack_all(
        Path::new(rs_pack::CONTENT_DIR),
        Path::new(rs_pack::PACK_DIR),
        true,
        true,
    )?;

    // The CRC table the client fetches first, to learn each archive's checksum.
    std::fs::write(out.join("crc"), &store.crctable_bytes[..])?;
    println!("crc                  {:>9} bytes", store.crctable_bytes.len());

    // Archive names are stable and known (rs-pack/src/lib.rs): title, config,
    // interface, media, versionlist, textures, wordenc, sounds.
    let mut names: Vec<&&str> = store.jags.keys().collect();
    names.sort();
    for name in names {
        let bytes = &store.jags[*name];
        std::fs::write(out.join(name), &bytes[..])?;
        let crc = store.crcs.get(*name).copied().unwrap_or(0);
        println!("{name:<20} {:>9} bytes  crc={crc}", bytes.len());
    }

    println!("\nwrote {} archives + crc to {}", store.jags.len(), out.display());
    Ok(())
}
