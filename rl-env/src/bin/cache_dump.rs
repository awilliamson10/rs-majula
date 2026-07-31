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

    // On-demand blobs: `Vec<Vec<Box<[u8]>>>` indexed [archive][file], matching
    // the client's `OnDemand.request(archive, file)`. Archives are
    // 0=models 1=anims 2=midi 3=maps. These are the wire bytes (gzip + a
    // 2-byte version suffix that the client strips in `OnDemand.loop`).
    let od = out.join("ondemand");
    let mut total = 0usize;
    let mut bytes = 0usize;
    for (archive, files) in store.ondemand.iter().enumerate() {
        let dir = od.join(archive.to_string());
        std::fs::create_dir_all(&dir)?;
        let mut present = 0usize;
        for (file, blob) in files.iter().enumerate() {
            if blob.is_empty() {
                continue; // versionlist marks these 0; the client never asks
            }
            std::fs::write(dir.join(format!("{file}.dat")), &blob[..])?;
            present += 1;
            bytes += blob.len();
        }
        println!("ondemand[{archive}]     {present:>6} files / {} slots", files.len());
        total += present;
    }

    println!(
        "\nwrote {} archives + crc + {total} ondemand files ({:.1} MB) to {}",
        store.jags.len(),
        bytes as f64 / 1e6,
        out.display()
    );
    Ok(())
}
