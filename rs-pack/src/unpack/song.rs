use std::path::Path;

use rs_io::bz2::bz2_decompress;
use tracing::info;

pub fn unpack_songs(songs_dir: &Path, output_dir: &Path) -> anyhow::Result<()> {
    let out_dir = output_dir.join("songs");
    std::fs::create_dir_all(&out_dir)?;

    let Ok(entries) = std::fs::read_dir(songs_dir) else {
        return Ok(());
    };

    let mut files: Vec<_> = entries.flatten().collect();
    files.sort_by_key(|e| e.file_name());

    let mut count = 0;
    for entry in files {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let key = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let data = std::fs::read(&path)?;
        if data.len() < 4 {
            continue;
        }

        let uncompressed_size = ((data[0] as u32) << 24)
            | ((data[1] as u32) << 16)
            | ((data[2] as u32) << 8)
            | (data[3] as u32);

        let midi = bz2_decompress(&data[4..], uncompressed_size as usize, true, 0);

        let mid_name = key_to_filename(&key);
        std::fs::write(out_dir.join(&mid_name), &midi)?;
        count += 1;
    }

    info!("Unpacked {} songs", count);
    Ok(())
}

fn key_to_filename(key: &str) -> String {
    if key.len() < 12
        && let Some(stem) = key.strip_suffix("_mid")
    {
        return format!("{stem}.mid");
    }
    format!("{key}.mid")
}
