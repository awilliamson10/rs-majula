pub enum RecolType {
    S(usize, u16),
    D(usize, u16),
}

pub fn rgb15_to_hsl16(rgb: u16) -> u16 {
    let r = ((rgb >> 10) & 0x1F) as f64 / 31.0;
    let g = ((rgb >> 5) & 0x1F) as f64 / 31.0;
    let b = (rgb & 0x1F) as f64 / 31.0;
    rgb_to_hsl(r, g, b)
}

fn rgb_to_hsl(r: f64, g: f64, b: f64) -> u16 {
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);

    let mut h_norm: f64 = 0.0;
    let mut s_norm: f64 = 0.0;
    let l_norm: f64 = (min + max) / 2.0;

    if (min - max).abs() > f64::EPSILON {
        if l_norm < 0.5 {
            s_norm = (max - min) / (max + min);
        } else {
            s_norm = (max - min) / (2.0 - max - min);
        }

        if (r - max).abs() < f64::EPSILON {
            h_norm = (g - b) / (max - min);
        } else if (g - max).abs() < f64::EPSILON {
            h_norm = (b - r) / (max - min) + 2.0;
        } else if (b - max).abs() < f64::EPSILON {
            h_norm = (r - g) / (max - min) + 4.0;
        }
    }

    h_norm /= 6.0;

    let h = (h_norm * 256.0) as i32 as u8;
    let s = ((s_norm * 256.0) as i32).clamp(0, 255) as u8;
    let l = ((l_norm * 256.0) as i32).clamp(0, 255) as u8;

    hsl24to16(h, s, l)
}

fn hsl24to16(hue: u8, mut saturation: u8, lightness: u8) -> u16 {
    if lightness > 243 {
        saturation >>= 4;
    } else if lightness > 217 {
        saturation >>= 3;
    } else if lightness > 192 {
        saturation >>= 2;
    } else if lightness > 179 {
        saturation >>= 1;
    }
    (((hue >> 2) as u16) << 10) | (((saturation >> 5) as u16) << 7) | ((lightness >> 1) as u16)
}

#[cfg(test)]
mod tests {
    use crate::pack::util::colour::rgb15_to_hsl16;

    #[test]
    fn test_rgb15_to_hsl16() {
        assert_eq!(rgb15_to_hsl16(32765), 10363);
        assert_eq!(rgb15_to_hsl16(7365), 5272);
        assert_eq!(rgb15_to_hsl16(7366), 26);
        assert_eq!(rgb15_to_hsl16(28371), 4446);
        assert_eq!(rgb15_to_hsl16(18858), 4409);
    }
}
