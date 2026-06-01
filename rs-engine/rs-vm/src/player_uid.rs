use rs_util::base37::{to_raw_username, to_screen_name, to_userhash};

/// A packed 128-bit unique identifier for a player, combining a base37 username hash
/// and an 11-bit player index into a single `u128` value.
///
/// Bit layout: `(username37 << 11) | (pid & 0x7FF)`
///
/// The upper bits store the base37-encoded username hash, while the lowest 11 bits
/// store the player index (0--2047), which corresponds to the player's slot in the
/// engine's fixed-size player array.
///
/// Used throughout the engine for player identification in scripts, queues, and
/// entity interactions.
#[derive(Debug, Copy, Clone)]
pub struct PlayerUid(u128);

impl PlayerUid {
    /// Constructs a new `PlayerUid` by packing the base37 username hash and player index.
    ///
    /// # Arguments
    /// * `username` - The player's username string. Converted to a base37 hash via
    ///   [`to_userhash`], so casing is irrelevant.
    /// * `pid` - The player index (0--2047). Only the lowest 11 bits are retained;
    ///   values above 2047 silently wrap (e.g. `0x800` becomes `0`).
    ///
    /// # Returns
    /// A `PlayerUid` whose packed representation is `(username37 << 11) | (pid & 0x7FF)`.
    ///
    /// # Call Stack
    /// **Calls:** [`to_userhash`]
    #[inline(always)]
    pub fn new(username: Box<str>, pid: u16) -> Self {
        Self(((to_userhash(&username) as u128) << 11) | (pid & 0x7FF) as u128)
    }

    /// Returns the raw packed `u128` value containing both the username hash and player index.
    ///
    /// # Returns
    /// The full 128-bit packed representation: `(username37 << 11) | pid`.
    #[inline(always)]
    pub const fn packed(&self) -> u128 {
        self.0
    }

    /// Extracts the base37 username hash from the packed representation.
    ///
    /// # Returns
    /// The `u64` base37-encoded username hash, obtained by right-shifting the packed
    /// value by 11 bits. This value can be passed to [`to_raw_username`] or
    /// [`to_screen_name`] to recover the original username string.
    #[inline(always)]
    pub const fn username37(&self) -> u64 {
        (self.0 >> 11) as u64
    }

    /// Extracts the 11-bit player index from the packed representation.
    ///
    /// # Returns
    /// The player index in the range `0..=2047`, used as an index into the engine's
    /// fixed-size player array (`players[MAX_PLAYERS]` where `MAX_PLAYERS = 2048`).
    #[inline(always)]
    pub const fn pid(&self) -> u16 {
        (self.0 & 0x7FF) as u16
    }

    /// Decodes the base37 username hash back into the raw lowercase username string.
    ///
    /// # Returns
    /// The player's username as a lowercase `String` (e.g. `"hello_world"`).
    ///
    /// # Call Stack
    /// **Calls:** [`username37`](Self::username37), [`to_raw_username`]
    pub fn username(&self) -> String {
        to_raw_username(self.username37())
    }

    /// Decodes the base37 username hash into a title-case display name suitable for
    /// rendering in the game client.
    ///
    /// Underscores in the raw username are converted to spaces and each word is
    /// capitalized (e.g. `"hello_world"` becomes `"Hello World"`).
    ///
    /// # Returns
    /// The player's display name as a title-cased `String`.
    ///
    /// # Call Stack
    /// **Calls:** [`username`](Self::username), [`to_screen_name`]
    pub fn screen_name(&self) -> String {
        to_screen_name(&self.username())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn new_and_pid() {
        let uid = PlayerUid::new("jordan".into(), 42);
        assert_eq!(uid.pid(), 42);
    }

    #[test]
    fn pid_max_11_bits() {
        let uid = PlayerUid::new("test".into(), 0x7FF);
        assert_eq!(uid.pid(), 0x7FF);
    }

    #[test]
    fn pid_wraps_past_11_bits() {
        let uid = PlayerUid::new("test".into(), 0x800);
        assert_eq!(uid.pid(), 0);
    }

    #[test]
    fn username_round_trip() {
        let uid = PlayerUid::new("jordan".into(), 1);
        assert_eq!(uid.username(), "jordan");
    }

    #[test]
    fn username37_consistent() {
        let uid = PlayerUid::new("hello".into(), 0);
        assert_eq!(uid.username37(), to_userhash("hello"));
    }

    #[test]
    fn screen_name() {
        let uid = PlayerUid::new("hello_world".into(), 0);
        assert_eq!(uid.screen_name(), "Hello World");
    }

    #[test]
    fn different_pids_same_username() {
        let a = PlayerUid::new("test".into(), 1);
        let b = PlayerUid::new("test".into(), 2);
        assert_eq!(a.username37(), b.username37());
        assert_ne!(a.pid(), b.pid());
    }

    #[test]
    fn pid_zero() {
        let uid = PlayerUid::new("test".into(), 0);
        assert_eq!(uid.pid(), 0);
    }

    #[test]
    fn copy_semantics() {
        let a = PlayerUid::new("test".into(), 5);
        let b = a;
        assert_eq!(b.pid(), 5);
        assert_eq!(b.username(), "test");
    }

    #[test]
    fn case_insensitive_username() {
        let a = PlayerUid::new("Jordan".into(), 1);
        let b = PlayerUid::new("jordan".into(), 1);
        assert_eq!(a.username37(), b.username37());
    }

    #[test]
    fn pid_max_valid_index() {
        // Engine uses pid as index into players[MAX_PLAYERS] where MAX_PLAYERS=2048
        // 11 bits = 0..2047
        let uid = PlayerUid::new("test".into(), 2047);
        assert_eq!(uid.pid(), 2047);
    }

    #[test]
    fn uid_packed_as_i32() {
        // Engine casts uid.0 as i32 for stack operations
        let uid = PlayerUid::new("test".into(), 1);
        let as_i32 = uid.0 as i32;
        // Extracting pid back from i32
        assert_eq!((as_i32 & 0x7FF) as u16, 1);
    }

    #[test]
    fn username_with_underscores() {
        let uid = PlayerUid::new("hello_world".into(), 0);
        assert_eq!(uid.username(), "hello_world");
    }

    #[test]
    fn username_with_digits() {
        let uid = PlayerUid::new("player123".into(), 0);
        assert_eq!(uid.username(), "player123");
    }

    #[test]
    fn screen_name_single_name() {
        let uid = PlayerUid::new("jordan".into(), 0);
        assert_eq!(uid.screen_name(), "Jordan");
    }

    #[test]
    fn many_pids_unique() {
        let mut pids = HashSet::new();
        for pid in 0..100u16 {
            let uid = PlayerUid::new("test".into(), pid);
            pids.insert(uid.pid());
        }
        assert_eq!(pids.len(), 100);
    }
}
