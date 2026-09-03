//! Npc spawn coordinates, parsed out of the `.jm2` map squares.
//!
//! ★★ COORDINATES IN A `.jm2` ARE LOCAL TO THE SQUARE. The filename carries the
//! square -- `m<mx>_<mz>.jm2` -- and absolute is `(mx * 64 + x, mz * 64 + z)`.
//! Forgetting the multiply produces coordinates that are plausible integers in
//! 0..63 and land the agent in the sea, which is why
//! `coordinates_are_absolute_not_local` asserts the maximum rather than any
//! specific answer.
//!
//! ★ Sections are `==== MAP ====` / `==== LOC ====` / `==== NPC ====` /
//! `==== OBJ ====`. Only NPC is read here; a loc scan would be the same shape
//! against a different header, and locs are how a task names a door or a tree.

use std::collections::BTreeMap;
use std::path::Path;

/// npc id -> every absolute `(x, level, z)` it is spawned at.
pub fn scan(maps_dir: &Path) -> BTreeMap<u16, Vec<(u16, u8, u16)>> {
    let mut out: BTreeMap<u16, Vec<(u16, u8, u16)>> = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(maps_dir) else { return out };
    let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    // ★ Sorted so the artifact is byte-stable: `read_dir` order is filesystem
    // order, which differs between machines and between runs.
    files.sort();

    for path in files {
        if path.extension().and_then(|s| s.to_str()) != Some("jm2") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Some(rest) = stem.strip_prefix('m') else { continue };
        let Some((mx, mz)) = rest.split_once('_') else { continue };
        let (Ok(mx), Ok(mz)) = (mx.parse::<u16>(), mz.parse::<u16>()) else { continue };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };

        let mut in_npc = false;
        for line in text.lines() {
            if line.starts_with("====") {
                in_npc = line.contains("NPC");
                continue;
            }
            if !in_npc {
                continue;
            }
            // `level x z: npcid`
            let Some((coords, id)) = line.split_once(':') else { continue };
            let Ok(id) = id.trim().parse::<u16>() else { continue };
            let mut it = coords.split_whitespace();
            let (Some(l), Some(x), Some(z)) = (it.next(), it.next(), it.next()) else { continue };
            let (Ok(l), Ok(x), Ok(z)) = (l.parse::<u8>(), x.parse::<u16>(), z.parse::<u16>())
            else {
                continue;
            };
            out.entry(id).or_default().push((mx * 64 + x, l, mz * 64 + z));
        }
    }
    out
}
