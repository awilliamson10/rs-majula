use crate::pack::pack_registry::PackRegistry;
use crate::types::{MapSquareCrcs, MapSquares};
use crate::unpack;
use rs_io::crc;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info};

type ExpectedMapSquare = (MapSquares, MapSquareCrcs);

pub fn verify_roundtrip(expected_dir: &Path, unpacked_dir: &Path) -> anyhow::Result<()> {
    let pack_dir = unpacked_dir.join("pack");
    let mut pass = 0u32;
    let mut fail = 0u32;

    info!("Verifying roundtrip: unpack → pack → CRC match");
    info!("  expected: {}", expected_dir.display());
    info!("  unpacked: {}", unpacked_dir.display());

    // Config
    if expected_dir.join("config").exists() {
        let expected_crc = file_crc(&expected_dir.join("config"));
        let raw_dir = unpacked_dir.join("_raw").join("config");
        let packed = unpack::pack_jag_from_raw(&raw_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("config", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Interface
    if expected_dir.join("interface").exists() {
        let expected_crc = file_crc(&expected_dir.join("interface"));
        let raw_dir = unpacked_dir.join("_raw").join("interface");
        let packed = unpack::pack_jag_from_raw(&raw_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("interface", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Media
    if expected_dir.join("media").exists() {
        let expected_crc = file_crc(&expected_dir.join("media"));
        let packed = crate::pack::media::pack_media_jag(unpacked_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("media", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Title
    if expected_dir.join("title").exists() {
        let expected_crc = file_crc(&expected_dir.join("title"));
        let packed = crate::pack::title::pack_title_jag(unpacked_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("title", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Textures
    if expected_dir.join("textures").exists() && pack_dir.exists() {
        let expected_crc = file_crc(&expected_dir.join("textures"));
        let registry = PackRegistry::load(&pack_dir)?;
        let packed = crate::pack::texture::pack_textures_jag(&registry, unpacked_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("textures", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Wordenc
    if expected_dir.join("wordenc").exists() {
        let expected_crc = file_crc(&expected_dir.join("wordenc"));
        let packed = crate::pack::wordenc::pack_wordenc(unpacked_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("wordenc", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Sounds
    if expected_dir.join("sounds").exists() && pack_dir.exists() {
        let expected_crc = file_crc(&expected_dir.join("sounds"));
        let registry = PackRegistry::load(&pack_dir)?;
        let packed = crate::pack::sound::pack_sounds(&registry, unpacked_dir, &pack_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("sounds", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Models
    if expected_dir.join("models").exists() {
        let expected_crc = file_crc(&expected_dir.join("models"));
        let raw_dir = unpacked_dir.join("models").join("_raw");
        let packed = super::model::pack_models_from_raw(&raw_dir);
        let actual_crc = crc::getcrc(&packed, 0, packed.len());
        check("models", expected_crc, actual_crc, &mut pass, &mut fail);
    }

    // Maps
    if expected_dir.join("maps").exists() {
        let expected_maps_dir = expected_dir.join("maps");
        let (_, packed_crcs, _, _) = crate::pack::other::map::pack_maps(unpacked_dir);
        let (_, expected_crcs) = load_expected_maps(&expected_maps_dir);

        let mut map_pass = 0u32;
        let mut map_fail = 0u32;
        for (key, expected_map_crc) in &expected_crcs {
            if let Some(&actual_map_crc) = packed_crcs.get(key) {
                if actual_map_crc == *expected_map_crc {
                    map_pass += 1;
                } else {
                    map_fail += 1;
                    error!(
                        "  maps/{}{}_{}:  FAIL (expected {}, got {})",
                        key.0, key.1, key.2, expected_map_crc, actual_map_crc
                    );
                }
            } else {
                map_fail += 1;
                error!("  maps/{}{}_{}:  MISSING", key.0, key.1, key.2);
            }
        }
        if map_fail == 0 {
            info!(
                "  maps:      PASS ({map_pass}/{} squares)",
                expected_crcs.len()
            );
            pass += 1;
        } else {
            error!(
                "  maps:      FAIL ({map_fail} mismatches out of {} squares)",
                expected_crcs.len()
            );
            fail += 1;
        }
    }

    // Songs
    if expected_dir.join("songs").exists() {
        let expected_songs_dir = expected_dir.join("songs");
        let packed_songs = crate::pack::other::song::pack_songs(unpacked_dir);

        let mut song_pass = 0u32;
        let mut song_fail = 0u32;
        let mut song_total = 0u32;

        let Ok(entries) = std::fs::read_dir(&expected_songs_dir) else {
            info!("  songs:     SKIP (cannot read expected dir)");
            return Ok(());
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let key = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            song_total += 1;
            let expected_data = std::fs::read(&path).unwrap_or_default();
            if let Some(packed_data) = packed_songs.get(&key) {
                if *packed_data == expected_data {
                    song_pass += 1;
                } else {
                    song_fail += 1;
                    error!("  songs/{key}: FAIL (data mismatch)");
                }
            } else {
                song_fail += 1;
                error!("  songs/{key}: MISSING");
            }
        }

        if song_fail == 0 {
            info!("  songs:     PASS ({song_pass}/{song_total})");
            pass += 1;
        } else {
            error!("  songs:     FAIL ({song_fail} mismatches out of {song_total})");
            fail += 1;
        }
    }

    info!("Roundtrip verification: {pass} passed, {fail} failed");
    if fail > 0 {
        anyhow::bail!("{fail} JAG type(s) failed CRC verification");
    }
    Ok(())
}

fn file_crc(path: &Path) -> i32 {
    let data = std::fs::read(path).expect("Cannot read expected file");
    crc::getcrc(&data, 0, data.len())
}

fn check(name: &str, expected: i32, actual: i32, pass: &mut u32, fail: &mut u32) {
    if actual == expected {
        info!("  {name:10} PASS (CRC: {expected})");
        *pass += 1;
    } else {
        error!("  {name:10} FAIL (expected {expected}, got {actual})");
        *fail += 1;
    }
}

fn load_expected_maps(maps_dir: &Path) -> ExpectedMapSquare {
    let mut data_map = HashMap::new();
    let mut crc_map = HashMap::new();

    let Ok(entries) = std::fs::read_dir(maps_dir) else {
        return (data_map, crc_map);
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let prefix = name.chars().next().unwrap_or(' ');
        let rest = &name[1..];
        let parts: Vec<&str> = rest.split('_').collect();
        if parts.len() != 2 {
            continue;
        }
        let Ok(x) = parts[0].parse::<u8>() else {
            continue;
        };
        let Ok(z) = parts[1].parse::<u8>() else {
            continue;
        };

        let file_data = std::fs::read(&path).unwrap_or_default();
        let file_crc = crc::getcrc(&file_data, 0, file_data.len());
        data_map.insert((prefix, x, z), Arc::from(file_data));
        crc_map.insert((prefix, x, z), file_crc);
    }

    (data_map, crc_map)
}
