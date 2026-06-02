use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn convert_image(index: &mut Vec<u8>, sprite_dir: &Path) -> Vec<u8> {
    let mut png_files: Vec<std::path::PathBuf> = std::fs::read_dir(sprite_dir)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", sprite_dir.display()))
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "png") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    png_files.sort_by_key(|p| {
        p.file_stem()
            .unwrap()
            .to_string_lossy()
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    let mut data = Vec::new();
    let index_pos = index.len() as u16;
    data.push((index_pos >> 8) as u8);
    data.push(index_pos as u8);

    if png_files.is_empty() {
        return data;
    }

    // Load all images once upfront.
    let images: Vec<image::RgbaImage> = png_files
        .iter()
        .map(|p| {
            image::open(p)
                .unwrap_or_else(|e| panic!("Failed to load {}: {e}", p.display()))
                .to_rgba8()
        })
        .collect();

    let first_img = &images[0];

    let mut strip_rows = 0u32;
    for y in (0..first_img.height()).rev() {
        if (0..first_img.width()).any(|x| first_img.get_pixel(x, y)[3] == 254) {
            strip_rows += 1;
        } else {
            break;
        }
    }

    let tile_w = first_img.width();
    let tile_h = first_img.height() - strip_rows;

    let mut palette = vec![0xFF00FFu32];
    if strip_rows > 0 {
        for sy in tile_h..first_img.height() {
            for sx in 0..first_img.width() {
                let px = first_img.get_pixel(sx, sy);
                if px[3] == 254 {
                    palette.push(((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32);
                }
            }
        }
    } else {
        let mut seen = HashSet::new();
        seen.insert(0xFF00FF);
        for img in &images {
            for y in 0..tile_h {
                for x in 0..tile_w {
                    let px = img.get_pixel(x, y);
                    if px[3] != 255 {
                        continue;
                    }
                    let rgb = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32;
                    if seen.insert(rgb) {
                        palette.push(rgb);
                    }
                }
            }
        }
        if palette.len() > 256 {
            palette.truncate(256);
        }
    }

    index.push((tile_w >> 8) as u8);
    index.push(tile_w as u8);
    index.push((tile_h >> 8) as u8);
    index.push(tile_h as u8);
    index.push(palette.len() as u8);
    for &c in palette.iter().skip(1) {
        index.push((c >> 16) as u8);
        index.push((c >> 8) as u8);
        index.push(c as u8);
    }

    let color_map: HashMap<u32, u8> = palette
        .iter()
        .enumerate()
        .map(|(i, &c)| (c, i as u8))
        .collect();

    for img in &images {
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut has_content = false;

        for y in 0..tile_h {
            for x in 0..tile_w {
                if img.get_pixel(x, y)[3] == 255 {
                    has_content = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        let (crop_x, crop_y, content_w, content_h) = if has_content {
            (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
        } else {
            (0, 0, 0, 0)
        };

        index.push(crop_x as u8);
        index.push(crop_y as u8);
        index.push((content_w >> 8) as u8);
        index.push(content_w as u8);
        index.push((content_h >> 8) as u8);
        index.push(content_h as u8);

        if content_w == 0 || content_h == 0 {
            index.push(0);
            continue;
        }

        let cw = content_w as usize;
        let ch = content_h as usize;
        let mut indices = vec![0u8; cw * ch];
        for y in 0..ch {
            for x in 0..cw {
                let px = img.get_pixel(crop_x + x as u32, crop_y + y as u32);
                let rgb = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32;
                indices[y * cw + x] = *color_map.get(&rgb).unwrap_or(&0);
            }
        }

        let mut row_runs: i64 = 0;
        let mut prev: i64 = -1;
        for idx in indices.iter().take(cw * ch) {
            let v = *idx as i64;
            if v == prev {
                row_runs += 1;
            }
            prev = v;
        }
        let mut col_runs: i64 = 0;
        prev = -1;
        for x in 0..cw {
            for y in 0..ch {
                let v = indices[y * cw + x] as i64;
                if v == prev {
                    col_runs += 1;
                }
                prev = v;
            }
        }
        let pixel_order: u8 = if col_runs > row_runs { 1 } else { 0 };
        index.push(pixel_order);

        if pixel_order == 0 {
            data.extend_from_slice(&indices);
        } else {
            for x in 0..cw {
                for y in 0..ch {
                    data.push(indices[y * cw + x]);
                }
            }
        }
    }

    data
}
