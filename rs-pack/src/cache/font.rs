use crate::types::Font;
use rs_io::jag::JagFile;

const CHAR_COUNT: usize = 94;

const CHAR_LOOKUP: [u8; 256] = {
    // £ (Latin-1 byte 163) at position 63 is handled separately below as \0 placeholder
    let charset: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!\"\xA3$%^&*()-_=+[{]};:'@#~,<.>/?\\| ";
    let charset_len = charset.len();
    let mut lookup = [74; 256];
    let mut i: usize = 0;
    while i < 256 {
        let mut j: usize = 0;
        while j < charset_len {
            if charset[j] == i as u8 {
                lookup[i] = j as u8;
                break;
            }
            j += 1;
        }
        i += 1;
    }
    lookup
};

pub struct FontType {
    pub id: u8,
    pub char_mask_width: [u8; CHAR_COUNT],
    pub char_mask_height: [u8; CHAR_COUNT],
    pub char_offset_x: [u8; CHAR_COUNT],
    pub char_offset_y: [u8; CHAR_COUNT],
    pub char_advance: [u8; CHAR_COUNT + 1],
    pub draw_width: [u8; 256],
    pub height: u16,
}

impl FontType {
    fn decode(id: u8, jag: &JagFile, name: &str) -> FontType {
        let mut data = jag.read(&format!("{name}.dat")).expect("missing font dat");
        let mut index = jag.read("index.dat").expect("missing index.dat");

        let mut font = FontType {
            id,
            char_mask_width: [0; CHAR_COUNT],
            char_mask_height: [0; CHAR_COUNT],
            char_offset_x: [0; CHAR_COUNT],
            char_offset_y: [0; CHAR_COUNT],
            char_advance: [0; CHAR_COUNT + 1],
            draw_width: [0; 256],
            height: 0,
        };

        index.pos = data.g2() as usize + 4;
        let pal_count = index.g1();
        if pal_count > 0 {
            index.pos += (pal_count as usize - 1) * 3;
        }

        for c in 0..CHAR_COUNT {
            font.char_offset_x[c] = index.g1();
            font.char_offset_y[c] = index.g1();

            let wi = index.g2() as usize;
            let hi = index.g2() as usize;
            font.char_mask_width[c] = wi as u8;
            font.char_mask_height[c] = hi as u8;

            let pixel_order = index.g1();
            let len = wi * hi;

            let mut mask = vec![0; len];
            if pixel_order == 0 {
                for slot in mask.iter_mut() {
                    *slot = data.g1();
                }
            } else if pixel_order == 1 {
                for x in 0..wi {
                    for y in 0..hi {
                        mask[x + y * wi] = data.g1();
                    }
                }
            }

            if hi as u16 > font.height {
                font.height = hi as u16;
            }

            font.char_offset_x[c] = 1;
            font.char_advance[c] = (wi + 2) as u8;

            if len > 0 {
                let mut space: u32 = 0;
                for y in (hi / 7)..hi {
                    space += mask[y * wi] as u32;
                }
                if space <= (hi / 7) as u32 {
                    font.char_advance[c] -= 1;
                    font.char_offset_x[c] = 0;
                }

                space = 0;
                for y in (hi / 7)..hi {
                    space += mask[wi - 1 + y * wi] as u32;
                }
                if space <= (hi / 7) as u32 {
                    font.char_advance[c] -= 1;
                }
            }
        }

        font.char_advance[94] = font.char_advance[8];

        for (i, &c) in CHAR_LOOKUP.iter().enumerate() {
            font.draw_width[i] = font.char_advance[c as usize];
        }

        font
    }

    pub fn string_width(&self, s: &str) -> u16 {
        let bytes = s.as_bytes();
        let mut size = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'@' && i + 4 < bytes.len() && bytes[i + 4] == b'@' {
                i += 5;
            } else {
                size += self.draw_width[bytes[i] as usize] as u16;
                i += 1;
            }
        }
        size
    }

    pub fn split(&self, s: &str, max_width: u16) -> Vec<String> {
        if s.is_empty() {
            return vec![s.to_string()];
        }

        let mut lines = Vec::new();
        let mut remaining = s;

        while !remaining.is_empty() {
            let width = self.string_width(remaining);
            if width <= max_width && !remaining.contains('|') {
                lines.push(remaining.to_string());
                break;
            }

            let mut split_index = remaining.len();
            let bytes = remaining.as_bytes();

            for i in 0..bytes.len() {
                if bytes[i] == b' ' {
                    let w = self.string_width(&remaining[..i]);
                    if w > max_width {
                        break;
                    }
                    split_index = i;
                } else if bytes[i] == b'|' {
                    split_index = i;
                    break;
                }
            }

            lines.push(remaining[..split_index].to_string());
            remaining = &remaining[split_index + 1..];
        }

        lines
    }
}

pub struct FontTypeProvider {
    pub fonts: Vec<FontType>,
}

impl FontTypeProvider {
    pub fn from_jag(jag_bytes: &[u8]) -> FontTypeProvider {
        let jag = JagFile::from(jag_bytes.to_vec());
        let fonts = Font::ALL
            .iter()
            .map(|font_id| FontType::decode(*font_id as u8, &jag, font_id.name()))
            .collect();
        FontTypeProvider { fonts }
    }

    pub fn get(&self, id: Font) -> Option<&FontType> {
        self.fonts.get(id as usize)
    }

    pub fn get_by_id(&self, id: u16) -> Option<&FontType> {
        self.fonts.get(id as usize)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&FontType> {
        self.get(Font::from_config_str(name))
    }

    pub fn count(&self) -> usize {
        self.fonts.len()
    }
}
