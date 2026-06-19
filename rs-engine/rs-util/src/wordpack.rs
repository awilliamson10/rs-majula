/// Frequency-ordered character lookup table for RuneScape chat message
/// compression.
///
/// The first 13 entries (indices 0--12) are encoded as single 4-bit nibbles.
/// Characters at index 13 and above require two nibbles (the index is offset
/// by 195 before being split across a high and low nibble pair). The ordering
/// places the most common English letters first to minimize packed message
/// size.
#[rustfmt::skip]
const CHAR_LOOKUP: [char; 61] = [
    ' ',
    'e', 't', 'a', 'o', 'i', 'h', 'n', 's', 'r', 'd', 'l', 'u', 'm',
    'w', 'c', 'y', 'f', 'g', 'p', 'b', 'v', 'k', 'x', 'j', 'q', 'z',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    ' ', '!', '?', '.', ',', ':', ';', '(', ')', '-',
    '&', '*', '\\', '\'', '@', '#', '+', '=', '£', '$', '%', '"', '[', ']'
];

/// Maximum number of characters an unpacked message may contain.
///
/// The [`unpack`] function stops emitting characters once this limit is
/// reached, and [`pack`] truncates its input to `MAX_LENGTH - 20` (80)
/// characters before encoding.
const MAX_LENGTH: usize = 100;

/// Decompresses a packed byte slice into a human-readable chat message string.
///
/// Each byte is split into two 4-bit nibbles (high then low). Nibble values
/// 0--12 map directly to [`CHAR_LOOKUP`] entries. Values 13--15 act as a
/// carry: they combine with the next nibble to form a two-nibble index
/// (`(carry << 4) + next - 195`) into the lookup table. Output is capped at
/// [`MAX_LENGTH`] characters, then sentence-cased via [`to_sentence_case`].
///
/// # Arguments
///
/// * `data` - The packed byte slice as received from the client protocol.
///
/// # Returns
///
/// A `String` containing the decompressed, sentence-cased message. An empty
/// slice produces an empty string.
///
/// # Side Effects
///
/// None.
///
/// # Panics
///
/// This function does not panic. Invalid lookup indices are silently skipped
/// via `CHAR_LOOKUP.get()`.
///
/// # Call Stack
///
/// **Called by:** Message handlers in `rs-engine/src/handlers/` for decoding
/// public and private chat messages.
///
/// **Calls:** [`to_sentence_case`].
pub fn unpack(data: &[u8]) -> String {
    fn nibble(out: &mut String, carry: &mut i32, nibble: i32) {
        if *carry != -1 {
            let idx = ((*carry << 4) + nibble - 195) as usize;
            if let Some(&c) = CHAR_LOOKUP.get(idx) {
                out.push(c);
            }
            *carry = -1;
        } else if nibble < 13 {
            out.push(CHAR_LOOKUP[nibble as usize]);
        } else {
            *carry = nibble;
        }
    }

    let mut out = String::with_capacity(MAX_LENGTH);
    let mut carry: i32 = -1;

    for &byte in data {
        if out.len() >= MAX_LENGTH {
            break;
        }

        let hi = ((byte >> 4) & 0xF) as i32;
        nibble(&mut out, &mut carry, hi);

        if out.len() >= MAX_LENGTH {
            break;
        }

        let lo = (byte & 0xF) as i32;
        nibble(&mut out, &mut carry, lo);
    }
    to_sentence_case(&out)
}

/// Compresses a chat message string into a packed byte vector.
///
/// The input is lowercased and truncated to 80 characters
/// (`MAX_LENGTH - 20`). Each character is looked up in [`CHAR_LOOKUP`];
/// characters not found default to index 0 (space). Indices 0--12 are stored
/// as single 4-bit nibbles; indices 13+ are offset by 195 and split across
/// two nibbles. Nibbles are packed pairwise into bytes; a trailing odd nibble
/// is left-shifted into the high nibble of a final byte.
///
/// # Arguments
///
/// * `input` - The raw chat message to compress. Characters outside
///   [`CHAR_LOOKUP`] are encoded as spaces.
///
/// # Returns
///
/// A `Vec<u8>` containing the packed representation. An empty input produces
/// an empty vector.
///
/// # Side Effects
///
/// None.
///
/// # Panics
///
/// This function does not panic.
///
/// # Call Stack
///
/// **Called by:** Message handlers in `rs-engine/src/handlers/` for encoding
/// public and private chat messages.
///
/// **Calls:** Nothing (leaf function, aside from std iterators).
pub fn pack(input: &str) -> Vec<u8> {
    let input: String = input
        .chars()
        .take(MAX_LENGTH - 20) // 80
        .flat_map(|c| c.to_lowercase())
        .collect();
    let mut out = Vec::new();
    let mut carry: i32 = -1;

    for ch in input.chars() {
        let mut index = CHAR_LOOKUP.iter().position(|&c| c == ch).unwrap_or(0) as i32;
        if index > 12 {
            index += 195;
        }
        if carry == -1 {
            if index < 13 {
                carry = index;
            } else {
                out.push(index as u8);
            }
        } else if index < 13 {
            out.push(((carry << 4) + index) as u8);
            carry = -1;
        } else {
            out.push(((carry << 4) + (index >> 4)) as u8);
            carry = index & 0xF;
        }
    }

    if carry != -1 {
        out.push((carry << 4) as u8);
    }

    out
}

/// Applies sentence-case formatting to a string: the first letter and any
/// letter immediately following a period (`.`) or exclamation mark (`!`) is
/// capitalized. All other characters are lowercased.
///
/// This mirrors the RuneScape client's chat normalization behavior, ensuring
/// that displayed messages begin with a capital letter and restart
/// capitalization after sentence-ending punctuation.
///
/// # Arguments
///
/// * `input` - The string to transform. May be any length.
///
/// # Returns
///
/// A new `String` with sentence-case applied. An empty input produces an
/// empty string.
///
/// # Side Effects
///
/// None.
///
/// # Panics
///
/// This function does not panic.
///
/// # Call Stack
///
/// **Called by:** [`unpack`] (applies sentence case to decompressed messages).
///
/// **Calls:** Nothing (leaf function, aside from std char methods).
pub fn to_sentence_case(input: &str) -> String {
    let mut chars: Vec<char> = input.to_lowercase().chars().collect();
    let mut punctuation = true;
    for c in &mut chars {
        if punctuation && c.is_ascii_lowercase() {
            *c = c.to_ascii_uppercase();
            punctuation = false;
        }
        if *c == '.' || *c == '!' {
            punctuation = true;
        }
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_round_trip() {
        let input = "Hello world";
        let packed = pack(input);
        let unpacked = unpack(&packed);
        assert_eq!(unpacked, "Hello world");
    }

    #[test]
    fn pack_unpack_with_punctuation() {
        let input = "Hello! How are you?";
        let packed = pack(input);
        let unpacked = unpack(&packed);
        assert_eq!(unpacked, "Hello! How are you? ");
    }

    #[test]
    fn pack_unpack_digits() {
        let input = "test 123";
        let packed = pack(input);
        let unpacked = unpack(&packed);
        assert_eq!(unpacked, "Test 123 ");
    }

    #[test]
    fn sentence_case_basic() {
        assert_eq!(to_sentence_case("hello world"), "Hello world");
    }

    #[test]
    fn sentence_case_after_period() {
        assert_eq!(to_sentence_case("hello. world"), "Hello. World");
    }

    #[test]
    fn sentence_case_after_exclamation() {
        assert_eq!(to_sentence_case("hello! world"), "Hello! World");
    }

    #[test]
    fn sentence_case_empty() {
        assert_eq!(to_sentence_case(""), "");
    }

    #[test]
    fn pack_empty() {
        let packed = pack("");
        assert!(packed.is_empty());
        let unpacked = unpack(&packed);
        assert_eq!(unpacked, "");
    }

    #[test]
    fn pack_truncates_at_80() {
        let long = "a".repeat(100);
        let packed = pack(&long);
        let unpacked = unpack(&packed);
        assert_eq!(unpacked.len(), 80);
    }

    #[test]
    fn pack_special_chars() {
        let input = "price: 100-200 (gp)";
        let packed = pack(input);
        let unpacked = unpack(&packed);
        assert_eq!(unpacked, "Price: 100-200 (gp) ");
    }

    #[test]
    fn unpack_empty_slice() {
        assert_eq!(unpack(&[]), "");
    }
}
