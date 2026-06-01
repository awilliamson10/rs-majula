/// Maximum number of heroes that can be tracked per entity.
const MAX_HEROES: usize = 16;

/// Represents a single hero entry, mapping a player's base37-encoded username
/// to a cumulative damage/contribution point total.
#[derive(Clone, Copy)]
struct Hero {
    user37: u64,
    points: i32,
}

impl Hero {
    /// Sentinel value representing an empty hero slot. The `user37` field is set
    /// to `u64::MAX` to distinguish it from any valid base37-encoded username,
    /// and `points` is initialized to 0.
    const EMPTY: Self = Self {
        user37: u64::MAX,
        points: 0,
    };
}

/// A fixed-capacity leaderboard that tracks up to [`MAX_HEROES`] (16) contributors
/// and their cumulative point totals for a single entity (player or NPC). Used to
/// determine which player dealt the most damage to an NPC for loot/XP attribution.
pub struct HeroPoints {
    heroes: [Hero; MAX_HEROES],
}

impl HeroPoints {
    /// Creates a new, empty [`HeroPoints`] with all slots initialized to [`Hero::EMPTY`].
    ///
    /// # Returns
    ///
    /// A [`HeroPoints`] instance with 16 empty hero slots.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `Player::new` (`rs-entity/src/player.rs`), `Npc::new` (`rs-entity/src/npc.rs`)
    /// **Calls:** None
    pub const fn new() -> Self {
        Self {
            heroes: [Hero::EMPTY; MAX_HEROES],
        }
    }

    /// Resets all hero slots back to [`Hero::EMPTY`], discarding all accumulated
    /// point data.
    ///
    /// # Side Effects
    ///
    /// * Overwrites the entire `heroes` array with empty sentinel values.
    ///
    /// # Call Stack
    ///
    /// **Called by:** Entity lifecycle reset paths
    /// **Calls:** None
    pub fn clear(&mut self) {
        self.heroes = [Hero::EMPTY; MAX_HEROES];
    }

    /// Adds `points` to the hero identified by `user37`. If the hero already exists
    /// in the leaderboard, their points are accumulated. If the hero is new and there
    /// is a free slot, a new entry is created. If there are no free slots, the call
    /// is silently dropped. Points less than 1 are ignored entirely.
    ///
    /// # Arguments
    ///
    /// * `user37` - The base37-encoded username of the contributing player.
    /// * `points` - The number of points to add. Values less than 1 are ignored.
    ///
    /// # Side Effects
    ///
    /// * Mutates an existing hero entry's `points` field if `user37` is already tracked.
    /// * Fills an empty slot with a new `Hero` entry if `user37` is not yet tracked.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `ScriptPlayer::add_hero` (`rs-engine/src/engine.rs`),
    ///   `ScriptNpc::add_hero` (`rs-engine/src/engine.rs`)
    /// **Calls:** None
    pub fn add_hero(&mut self, user37: u64, points: i32) {
        if points < 1 {
            return;
        }
        if let Some(hero) = self.heroes.iter_mut().find(|h| h.user37 == user37) {
            hero.points += points;
            return;
        }
        if let Some(hero) = self.heroes.iter_mut().find(|h| h.user37 == u64::MAX) {
            *hero = Hero { user37, points };
        }
    }

    /// Finds the hero with the highest cumulative points and returns their
    /// base37-encoded username. Sorts a cloned copy of the internal array in
    /// descending point order using [`quicksort`] and returns the top entry.
    ///
    /// # Returns
    ///
    /// * `Some(u64)` - The base37-encoded username of the hero with the most points.
    /// * `None` - If no heroes have been added (all slots are empty).
    ///
    /// # Call Stack
    ///
    /// **Called by:** `ScriptPlayer::find_hero` (`rs-engine/src/engine.rs`),
    ///   `ScriptNpc::find_hero` (`rs-engine/src/engine.rs`)
    /// **Calls:** [`quicksort`]
    pub fn find_hero(&self) -> Option<u64> {
        let mut clone = self.heroes;
        quicksort(&mut clone, |a, b| b.points - a.points);
        if clone[0].user37 == u64::MAX {
            None
        } else {
            Some(clone[0].user37)
        }
    }
}

/// Sorts a mutable slice of [`Hero`] entries using the quicksort algorithm with
/// a custom comparator. This is the entry point that delegates to [`quicksort_inner`]
/// for the recursive partitioning.
///
/// # Arguments
///
/// * `arr` - The mutable slice of [`Hero`] entries to sort in-place.
/// * `compare` - A comparison function that returns a negative value if `a` should
///   come before `b`, a positive value if `b` should come before `a`, and zero if
///   they are equal.
///
/// # Side Effects
///
/// * Sorts `arr` in-place according to the comparator.
///
/// # Call Stack
///
/// **Called by:** [`HeroPoints::find_hero`]
/// **Calls:** [`quicksort_inner`]
fn quicksort(arr: &mut [Hero], compare: fn(&Hero, &Hero) -> i32) {
    let len = arr.len();
    if len > 1 {
        quicksort_inner(0, len - 1, arr, compare);
    }
}

/// Recursive inner implementation of quicksort that partitions and sorts a subrange
/// of the [`Hero`] slice. Uses a middle-element pivot strategy with a stability-breaking
/// tiebreaker based on index parity (`loop_index & 1`), which introduces a deterministic
/// but non-stable ordering for elements with equal comparison values.
///
/// # Arguments
///
/// * `low` - The inclusive lower bound index of the subrange to sort.
/// * `high` - The inclusive upper bound index of the subrange to sort.
/// * `arr` - The mutable slice of [`Hero`] entries being sorted.
/// * `compare` - A comparison function that returns a negative value if `a` should
///   come before `b`, positive if `b` should come before `a`.
///
/// # Side Effects
///
/// * Reorders elements in `arr[low..=high]` in-place.
///
/// # Panics
///
/// * Panics if `low` or `high` are out of bounds for `arr` (via array indexing).
/// * Panics on arithmetic underflow if `counter` is 0 and the subtraction
///   `counter - 1` wraps (guarded by the `counter >= 1` check).
///
/// # Call Stack
///
/// **Called by:** [`quicksort`], [`quicksort_inner`] (recursive)
/// **Calls:** [`quicksort_inner`] (recursive), `compare` (function pointer)
fn quicksort_inner(low: usize, high: usize, arr: &mut [Hero], compare: fn(&Hero, &Hero) -> i32) {
    let pivot_index = (low + high) / 2;
    let pivot_value = arr[pivot_index];
    arr.swap(pivot_index, high);
    let mut counter = low;
    let mut loop_index = low;

    while loop_index < high {
        if compare(&arr[loop_index], &pivot_value) < (loop_index & 1) as i32 {
            arr.swap(loop_index, counter);
            counter += 1;
        }
        loop_index += 1;
    }

    arr[high] = arr[counter];
    arr[counter] = pivot_value;

    if counter >= 1 && low < counter - 1 {
        quicksort_inner(low, counter - 1, arr, compare);
    }
    if counter + 1 < high {
        quicksort_inner(counter + 1, high, arr, compare);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let hp = HeroPoints::new();
        assert!(hp.find_hero().is_none());
    }

    #[test]
    fn add_and_find() {
        let mut hp = HeroPoints::new();
        hp.add_hero(100, 5);
        assert_eq!(hp.find_hero(), Some(100));
    }

    #[test]
    fn highest_points_wins() {
        let mut hp = HeroPoints::new();
        hp.add_hero(1, 3);
        hp.add_hero(2, 10);
        hp.add_hero(3, 5);
        assert_eq!(hp.find_hero(), Some(2));
    }

    #[test]
    fn accumulates_points() {
        let mut hp = HeroPoints::new();
        hp.add_hero(1, 3);
        hp.add_hero(2, 10);
        hp.add_hero(1, 8);
        assert_eq!(hp.find_hero(), Some(1));
    }

    #[test]
    fn ignores_zero_or_negative() {
        let mut hp = HeroPoints::new();
        hp.add_hero(1, 0);
        hp.add_hero(2, -5);
        assert!(hp.find_hero().is_none());
    }

    #[test]
    fn clear_resets() {
        let mut hp = HeroPoints::new();
        hp.add_hero(1, 10);
        hp.clear();
        assert!(hp.find_hero().is_none());
    }

    #[test]
    fn single_hero() {
        let mut hp = HeroPoints::new();
        hp.add_hero(42, 1);
        assert_eq!(hp.find_hero(), Some(42));
    }

    #[test]
    fn many_heroes_sorted() {
        let mut hp = HeroPoints::new();
        for i in 0..16u64 {
            hp.add_hero(i, (i + 1) as i32);
        }
        assert_eq!(hp.find_hero(), Some(15));
    }
}
