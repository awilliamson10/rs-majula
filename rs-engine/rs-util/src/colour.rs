/// Converts a 24-bit RGB color (8 bits per channel) to a 15-bit RGB color
/// (5 bits per channel).
///
/// Each 8-bit channel is reduced to 5 bits by discarding the 3
/// least-significant bits (right-shift by 3). The resulting 5-bit channels are
/// packed into a `u16` in R(15:10) G(9:5) B(4:0) order. Values below 8 in any
/// channel will truncate to 0 due to the bit shift.
///
/// # Arguments
///
/// * `rgb` - A 24-bit RGB color packed in the lower 24 bits of an `i32`,
///   laid out as `0x00RRGGBB`. Negative values are treated as their
///   bit-equivalent unsigned representation (e.g. `-1` behaves like
///   `0xFFFFFFFF`, producing white).
///
/// # Returns
///
/// A `u16` in the range `0x0000..=0x7FFF` representing the 15-bit color.
///
/// # Side Effects
///
/// None. This is a pure `const fn`.
///
/// # Panics
///
/// This function does not panic.
///
/// # Call Stack
///
/// **Called by:** Player info rendering in `rs-info` for chat color encoding.
///
/// **Calls:** Nothing (leaf function).
#[inline(always)]
pub const fn rgb24_to_15(rgb: i32) -> u16 {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    (((r >> 3) << 10) + ((g >> 3) << 5) + (b >> 3)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black() {
        assert_eq!(rgb24_to_15(0x000000), 0);
    }

    #[test]
    fn white() {
        assert_eq!(rgb24_to_15(0xFFFFFF), 0x7FFF);
    }

    #[test]
    fn pure_red() {
        let result = rgb24_to_15(0xFF0000);
        assert_eq!(result, 0x1F << 10);
    }

    #[test]
    fn pure_green() {
        let result = rgb24_to_15(0x00FF00);
        assert_eq!(result, 0x1F << 5);
    }

    #[test]
    fn pure_blue() {
        let result = rgb24_to_15(0x0000FF);
        assert_eq!(result, 0x1F);
    }

    #[test]
    fn mid_gray() {
        let result = rgb24_to_15(0x808080);
        let r = (0x80 >> 3) as u16;
        let g = (0x80 >> 3) as u16;
        let b = (0x80 >> 3) as u16;
        assert_eq!(result, (r << 10) + (g << 5) + b);
    }

    #[test]
    fn specific_color() {
        let result = rgb24_to_15(0x1F3F5F);
        let r = (0x1F >> 3) as u16;
        let g = (0x3F >> 3) as u16;
        let b = (0x5F >> 3) as u16;
        assert_eq!(result, (r << 10) + (g << 5) + b);
    }

    #[test]
    fn low_values_truncate_to_zero() {
        assert_eq!(rgb24_to_15(0x070707), 0);
    }

    #[test]
    fn just_above_threshold() {
        let result = rgb24_to_15(0x080808);
        assert_eq!(result, (1 << 10) + (1 << 5) + 1);
    }

    #[test]
    fn yellow() {
        let result = rgb24_to_15(0xFFFF00);
        assert_eq!(result, (0x1F << 10) + (0x1F << 5));
    }

    #[test]
    fn cyan() {
        let result = rgb24_to_15(0x00FFFF);
        assert_eq!(result, (0x1F << 5) + 0x1F);
    }

    #[test]
    fn magenta() {
        let result = rgb24_to_15(0xFF00FF);
        assert_eq!(result, (0x1F << 10) + 0x1F);
    }

    #[test]
    fn game_ui_colors() {
        // Common RuneScape UI colors
        let red_chat = rgb24_to_15(0xFF0000);
        let green_chat = rgb24_to_15(0x00FF00);
        assert_ne!(red_chat, green_chat);
    }

    #[test]
    fn negative_rgb_treated_as_unsigned() {
        // Engine casts i32, negative values may appear
        let result = rgb24_to_15(-1); // 0xFFFFFFFF
        assert_eq!(result, 0x7FFF); // same as white
    }

    #[test]
    fn partial_channel_values() {
        // Values that aren't multiples of 8 lose precision
        let c1 = rgb24_to_15(0x100000);
        let c2 = rgb24_to_15(0x170000);
        assert_eq!(c1, c2); // both r=2 after >>3
    }

    #[test]
    fn max_individual_channels() {
        let r_max = rgb24_to_15(0xF80000); // r=31 exactly
        assert_eq!(r_max, 31 << 10);
        let g_max = rgb24_to_15(0x00F800);
        assert_eq!(g_max, 31 << 5);
        let b_max = rgb24_to_15(0x0000F8);
        assert_eq!(b_max, 31);
    }
}
