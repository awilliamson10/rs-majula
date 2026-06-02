pub mod config;
pub mod map;
pub mod media;
pub mod model;
pub mod song;
pub mod sound;
pub mod texture;
pub mod title;
pub mod verify;
pub mod wordenc;

use std::collections::HashMap;
use std::path::Path;

use image::{Rgba, RgbaImage};
use rs_io::Packet;
use rs_io::jag::JagFile;
use tracing::info;

const CONFIG_ENTRY_NAMES: &[&str] = &[
    "seq.dat",
    "seq.idx",
    "loc.dat",
    "loc.idx",
    "flo.dat",
    "flo.idx",
    "spotanim.dat",
    "spotanim.idx",
    "obj.dat",
    "obj.idx",
    "npc.dat",
    "npc.idx",
    "idk.dat",
    "idk.idx",
    "varp.dat",
    "varp.idx",
];

const INTERFACE_ENTRY_NAMES: &[&str] = &["data"];

pub fn unpack_all(expected_dir: &Path, output_dir: &Path, pack_dir: &Path) -> anyhow::Result<()> {
    info!("Unpacking assets...");
    info!("  expected: {}", expected_dir.display());
    info!("  output:   {}", output_dir.display());

    std::fs::create_dir_all(output_dir)?;
    std::fs::create_dir_all(pack_dir)?;

    // Config must run first - it produces model_categories needed by models.
    let mut model_categories = HashMap::new();
    let config_path = expected_dir.join("config");
    if config_path.exists() {
        info!("Unpacking config...");
        let jag = JagFile::from(std::fs::read(&config_path)?);
        let packs = config::unpack_config(&jag, output_dir, pack_dir)?;
        model_categories = packs.model_categories;
        let raw_dir = output_dir.join("_raw").join("config");
        let entries = dump_jag_entries(&jag, &raw_dir, CONFIG_ENTRY_NAMES)?;
        info!("  Dumped {} raw config entries", entries.len());
    }

    // Everything else is independent - run in parallel.
    let interface_path = expected_dir.join("interface");
    let media_path = expected_dir.join("media");
    let textures_path = expected_dir.join("textures");
    let title_path = expected_dir.join("title");
    let models_path = expected_dir.join("models");
    let sounds_path = expected_dir.join("sounds");
    let wordenc_path = expected_dir.join("wordenc");
    let songs_dir = expected_dir.join("songs");
    let maps_dir = expected_dir.join("maps");

    std::thread::scope(|s| {
        if interface_path.exists() {
            s.spawn(|| {
                info!("Unpacking interface...");
                let jag = JagFile::from(std::fs::read(&interface_path).unwrap());
                let raw_dir = output_dir.join("_raw").join("interface");
                let entries = dump_jag_entries(&jag, &raw_dir, INTERFACE_ENTRY_NAMES).unwrap();
                info!("  Dumped {} raw interface entries", entries.len());
            });
        }

        if media_path.exists() {
            s.spawn(|| {
                info!("Unpacking media...");
                let jag = JagFile::from(std::fs::read(&media_path).unwrap());
                media::unpack_media(&jag, output_dir).unwrap();
            });
        }

        if textures_path.exists() {
            s.spawn(|| {
                info!("Unpacking textures...");
                let jag = JagFile::from(std::fs::read(&textures_path).unwrap());
                texture::unpack_textures(&jag, output_dir, pack_dir).unwrap();
            });
        }

        if title_path.exists() {
            s.spawn(|| {
                info!("Unpacking title...");
                let jag = JagFile::from(std::fs::read(&title_path).unwrap());
                title::unpack_title(&jag, output_dir).unwrap();
            });
        }

        if models_path.exists() {
            s.spawn(|| {
                info!("Unpacking models...");
                let jag = JagFile::from(std::fs::read(&models_path).unwrap());
                model::unpack_models(&jag, output_dir, pack_dir, &model_categories).unwrap();
            });
        }

        if sounds_path.exists() {
            s.spawn(|| {
                info!("Unpacking sounds...");
                let jag = JagFile::from(std::fs::read(&sounds_path).unwrap());
                sound::unpack_sounds(&jag, output_dir, pack_dir).unwrap();
            });
        }

        if wordenc_path.exists() {
            s.spawn(|| {
                info!("Unpacking wordenc...");
                let jag = JagFile::from(std::fs::read(&wordenc_path).unwrap());
                wordenc::unpack_wordenc(&jag, output_dir).unwrap();
            });
        }

        if songs_dir.exists() {
            s.spawn(|| {
                info!("Unpacking songs...");
                song::unpack_songs(&songs_dir, output_dir).unwrap();
            });
        }

        if maps_dir.exists() {
            s.spawn(|| {
                info!("Unpacking maps...");
                map::unpack_maps(&maps_dir, output_dir).unwrap();
            });
        }
    });

    info!("Unpack complete.");
    Ok(())
}

pub fn decode_sprite_group(
    index_data: &[u8],
    dat_data: &[u8],
    output_dir: &Path,
) -> anyhow::Result<()> {
    let mut dat = Packet::from(dat_data.to_vec());

    let index_pos = dat.g2() as usize;
    if index_pos >= index_data.len() {
        return Ok(());
    }

    let mut idx = Packet::from(index_data[index_pos..].to_vec());

    let tile_w = idx.g2() as u32;
    let tile_h = idx.g2() as u32;
    let palette_len = idx.g1() as usize;

    let mut palette = vec![0xFF00FF];
    for _ in 1..palette_len {
        let r = idx.g1() as u32;
        let g = idx.g1() as u32;
        let b = idx.g1() as u32;
        palette.push((r << 16) | (g << 8) | b);
    }

    let strip_pixels = palette.len() - 1;
    let strip_rows = if tile_w > 0 {
        (strip_pixels as u32).div_ceil(tile_w)
    } else {
        0
    };

    let mut sprite_index = 0u32;
    while dat.remaining() > 0 {
        let crop_x = idx.g1() as u32;
        let crop_y = idx.g1() as u32;
        let content_w = idx.g2() as u32;
        let content_h = idx.g2() as u32;
        let pixel_order = idx.g1();

        let img_h = if sprite_index == 0 {
            tile_h + strip_rows
        } else {
            tile_h
        };
        let mut img = RgbaImage::new(tile_w, img_h);

        for y in 0..img_h {
            for x in 0..tile_w {
                img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }

        if content_w > 0 && content_h > 0 {
            let pixel_count = (content_w * content_h) as usize;
            let mut indices = vec![0u8; pixel_count];

            if pixel_order == 0 {
                for i in 0..pixel_count {
                    indices[i] = dat.g1();
                }
            } else {
                for x in 0..content_w as usize {
                    for y in 0..content_h as usize {
                        indices[y * content_w as usize + x] = dat.g1();
                    }
                }
            }

            for y in 0..content_h {
                for x in 0..content_w {
                    let pi = indices[(y * content_w + x) as usize] as usize;
                    let rgb = palette[pi.min(palette.len() - 1)];
                    let r = (rgb >> 16) as u8;
                    let g = (rgb >> 8) as u8;
                    let b = rgb as u8;
                    img.put_pixel(crop_x + x, crop_y + y, Rgba([r, g, b, 255]));
                }
            }
        }

        if sprite_index == 0 && strip_rows > 0 {
            let mut pi = 1;
            for sy in tile_h..tile_h + strip_rows {
                for sx in 0..tile_w {
                    if pi < palette.len() {
                        let rgb = palette[pi];
                        let r = (rgb >> 16) as u8;
                        let g = (rgb >> 8) as u8;
                        let b = rgb as u8;
                        img.put_pixel(sx, sy, Rgba([r, g, b, 254]));
                        pi += 1;
                    }
                }
            }
        }

        let out_path = output_dir.join(format!("{sprite_index}.png"));
        img.save(&out_path)?;
        sprite_index += 1;
    }

    Ok(())
}

pub fn dump_jag_entries(
    jag: &JagFile,
    output_dir: &Path,
    names: &[&str],
) -> anyhow::Result<Vec<String>> {
    std::fs::create_dir_all(output_dir)?;

    let mut jag_order = Vec::new();
    for i in 0..jag.file_count {
        let hash = jag.file_hash(i);
        for &name in names {
            if JagFile::hash(name) == hash {
                jag_order.push(name.to_string());
                break;
            }
        }
    }

    for name in &jag_order {
        if let Some(data) = jag.read(name) {
            std::fs::write(output_dir.join(name), &data.data)?;
        }
    }

    std::fs::write(
        output_dir.join("_jag_order.txt"),
        jag_order.join("\n") + "\n",
    )?;
    Ok(jag_order)
}

pub fn pack_jag_from_raw(raw_dir: &Path) -> Vec<u8> {
    let order: Vec<String> = std::fs::read_to_string(raw_dir.join("_jag_order.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    let mut jag = JagFile::new();
    for name in &order {
        if let Ok(data) = std::fs::read(raw_dir.join(name)) {
            jag.write(name, data);
        }
    }
    jag.build()
}
