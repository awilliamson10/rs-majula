use rs_vm::state::{ScriptArgument, TimedScript, TimerPriority};
use rustc_hash::FxHashMap;

/// A dual-lane timer registry that maps script IDs to [`TimedScript`] entries,
/// separated by [`TimerPriority`]. Normal timers fire during the main player/NPC
/// phase, while Soft timers fire during the soft timer sub-phase. Each script ID
/// can have at most one timer per priority; re-adding replaces the existing entry.
pub struct ScriptTimer {
    pub normal: FxHashMap<i32, TimedScript>,
    pub soft: FxHashMap<i32, TimedScript>,
}

impl ScriptTimer {
    /// Creates a new [`ScriptTimer`] with both lanes empty.
    ///
    /// # Returns
    ///
    /// A [`ScriptTimer`] with empty `normal` and `soft` hash maps.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `Player::new` (`rs-entity/src/player.rs`)
    /// **Calls:** [`FxHashMap::default`]
    pub fn new() -> Self {
        Self {
            normal: FxHashMap::default(),
            soft: FxHashMap::default(),
        }
    }

    /// Registers or replaces a timer for the given script ID and priority.
    /// If a timer with the same `script_id` and `priority` already exists,
    /// it is overwritten with the new interval, clock, and args.
    ///
    /// # Arguments
    ///
    /// * `priority` - Determines which lane (`normal` or `soft`) the timer is stored in.
    /// * `script_id` - The script identifier. Used as the hash map key.
    /// * `interval` - Number of ticks between firings.
    /// * `clock` - The game tick at which the timer was set (used to compute readiness).
    /// * `args` - Optional typed arguments passed to the script on each firing.
    ///
    /// # Side Effects
    ///
    /// * Inserts or replaces a [`TimedScript`] entry in the appropriate hash map.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `ScriptPlayer::settimer` (`rs-engine/src/engine.rs`),
    ///   `ScriptNpc::settimer` (`rs-engine/src/engine.rs`)
    /// **Calls:** [`FxHashMap::insert`]
    pub fn add(
        &mut self,
        priority: TimerPriority,
        script_id: i32,
        interval: u16,
        clock: u32,
        args: Option<Vec<ScriptArgument>>,
    ) {
        let timer = TimedScript {
            clock,
            args,
            script_id,
            interval,
            priority,
        };
        match priority {
            TimerPriority::Normal => self.normal.insert(script_id, timer),
            TimerPriority::Soft => self.soft.insert(script_id, timer),
        };
    }

    /// Removes the timer for the given script ID and priority, if one exists.
    /// Removing a non-existent timer is a no-op.
    ///
    /// # Arguments
    ///
    /// * `script_id` - The script identifier to remove.
    /// * `priority` - Which lane (`normal` or `soft`) to remove from.
    ///
    /// # Side Effects
    ///
    /// * Removes the [`TimedScript`] entry from the appropriate hash map if present.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `ScriptPlayer::clear_timer` (`rs-engine/src/engine.rs`)
    /// **Calls:** [`FxHashMap::remove`]
    pub fn remove(&mut self, script_id: i32, priority: TimerPriority) {
        match priority {
            TimerPriority::Normal => self.normal.remove(&script_id),
            TimerPriority::Soft => self.soft.remove(&script_id),
        };
    }

    /// Removes the timer for the given script ID from both lanes, if present.
    /// Removing a non-existent timer is a no-op.
    ///
    /// # Arguments
    ///
    /// * `script_id` - The script identifier to remove from the normal and
    ///   soft lanes.
    ///
    /// # Side Effects
    ///
    /// * Removes the matching [`TimedScript`] entry from `normal` and/or `soft`.
    pub fn remove_any(&mut self, script_id: i32) {
        self.normal.remove(&script_id);
        self.soft.remove(&script_id);
    }

    /// Returns the timer for the given script ID, checking the normal lane
    /// first and then the soft lane.
    ///
    /// A script ID has at most one timer per lane; when present in both, the
    /// normal-lane timer is returned.
    ///
    /// # Arguments
    ///
    /// * `script_id` - The script identifier to look up.
    ///
    /// # Returns
    ///
    /// `Some(&TimedScript)` if a timer exists in either lane, `None` otherwise.
    pub fn get(&self, script_id: i32) -> Option<&TimedScript> {
        self.normal
            .get(&script_id)
            .or_else(|| self.soft.get(&script_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_timer() {
        let t = ScriptTimer::new();
        assert!(t.normal.is_empty());
        assert!(t.soft.is_empty());
    }

    #[test]
    fn add_normal_timer() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        assert_eq!(t.normal.len(), 1);
        assert!(t.soft.is_empty());
        let timer = t.normal.get(&1).unwrap();
        assert_eq!(timer.script_id, 1);
        assert_eq!(timer.interval, 10);
        assert_eq!(timer.clock, 100);
        assert!(timer.args.is_none());
    }

    #[test]
    fn add_soft_timer() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Soft, 2, 20, 200, None);
        assert!(t.normal.is_empty());
        assert_eq!(t.soft.len(), 1);
        let timer = t.soft.get(&2).unwrap();
        assert_eq!(timer.script_id, 2);
        assert_eq!(timer.interval, 20);
        assert_eq!(timer.clock, 200);
    }

    #[test]
    fn add_with_args() {
        let mut t = ScriptTimer::new();
        let args = vec![
            ScriptArgument::Int(42),
            ScriptArgument::String("hello".into()),
        ];
        t.add(TimerPriority::Normal, 1, 5, 50, Some(args));
        let timer = t.normal.get(&1).unwrap();
        let args = timer.args.as_ref().unwrap();
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn add_replaces_existing_same_id() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.add(TimerPriority::Normal, 1, 20, 200, None);
        assert_eq!(t.normal.len(), 1);
        let timer = t.normal.get(&1).unwrap();
        assert_eq!(timer.interval, 20);
        assert_eq!(timer.clock, 200);
    }

    #[test]
    fn add_multiple_different_ids() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.add(TimerPriority::Normal, 2, 20, 200, None);
        t.add(TimerPriority::Normal, 3, 30, 300, None);
        assert_eq!(t.normal.len(), 3);
    }

    #[test]
    fn remove_normal_timer() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.remove(1, TimerPriority::Normal);
        assert!(t.normal.is_empty());
    }

    #[test]
    fn remove_soft_timer() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Soft, 2, 20, 200, None);
        t.remove(2, TimerPriority::Soft);
        assert!(t.soft.is_empty());
    }

    #[test]
    fn remove_nonexistent_no_panic() {
        let mut t = ScriptTimer::new();
        t.remove(999, TimerPriority::Normal);
        t.remove(999, TimerPriority::Soft);
    }

    #[test]
    fn remove_wrong_priority_doesnt_remove() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.remove(1, TimerPriority::Soft);
        assert_eq!(t.normal.len(), 1);
    }

    #[test]
    fn same_id_different_priorities() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.add(TimerPriority::Soft, 1, 20, 200, None);
        assert_eq!(t.normal.len(), 1);
        assert_eq!(t.soft.len(), 1);
        assert_eq!(t.normal.get(&1).unwrap().interval, 10);
        assert_eq!(t.soft.get(&1).unwrap().interval, 20);
    }

    #[test]
    fn remove_one_priority_keeps_other() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.add(TimerPriority::Soft, 1, 20, 200, None);
        t.remove(1, TimerPriority::Normal);
        assert!(t.normal.is_empty());
        assert_eq!(t.soft.len(), 1);
    }

    #[test]
    fn priority_field_matches() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);
        t.add(TimerPriority::Soft, 2, 20, 200, None);
        assert_eq!(t.normal.get(&1).unwrap().priority, TimerPriority::Normal);
        assert_eq!(t.soft.get(&2).unwrap().priority, TimerPriority::Soft);
    }

    #[test]
    fn add_many_and_remove_all() {
        let mut t = ScriptTimer::new();
        for i in 0..100 {
            t.add(TimerPriority::Normal, i, i as u16, i as u32, None);
        }
        assert_eq!(t.normal.len(), 100);
        for i in 0..100 {
            t.remove(i, TimerPriority::Normal);
        }
        assert!(t.normal.is_empty());
    }

    #[test]
    fn negative_script_ids() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, -1, 10, 100, None);
        t.add(TimerPriority::Normal, -100, 20, 200, None);
        assert_eq!(t.normal.len(), 2);
        assert!(t.normal.contains_key(&-1));
        assert!(t.normal.contains_key(&-100));
        t.remove(-1, TimerPriority::Normal);
        assert_eq!(t.normal.len(), 1);
    }

    #[test]
    fn zero_interval_and_clock() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 0, 0, None);
        let timer = t.normal.get(&1).unwrap();
        assert_eq!(timer.interval, 0);
        assert_eq!(timer.clock, 0);
    }

    #[test]
    fn max_values() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, i32::MAX, u16::MAX, u32::MAX, None);
        let timer = t.normal.get(&i32::MAX).unwrap();
        assert_eq!(timer.interval, u16::MAX);
        assert_eq!(timer.clock, u32::MAX);
    }

    #[test]
    fn timer_ready_check_pattern() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);

        let timer = t.normal.get(&1).unwrap();
        let current_clock: u32 = 110;
        let ready = current_clock >= timer.clock + timer.interval as u32;
        assert!(ready);
    }

    #[test]
    fn timer_not_ready_check() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);

        let timer = t.normal.get(&1).unwrap();
        let current_clock: u32 = 105;
        let ready = current_clock >= timer.clock + timer.interval as u32;
        assert!(!ready);
    }

    #[test]
    fn timer_update_clock_after_fire() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 100, None);

        // Simulate firing at clock 110
        let timer = t.normal.get_mut(&1).unwrap();
        timer.clock = 110; // update clock to fire time
        assert_eq!(timer.clock, 110);
    }

    #[test]
    fn iterate_and_collect_ready_timers() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 5, 0, None);
        t.add(TimerPriority::Normal, 2, 10, 0, None);
        t.add(TimerPriority::Normal, 3, 3, 0, None);

        let current_clock: u32 = 5;
        let ready: Vec<i32> = t
            .normal
            .iter()
            .filter(|(_, timer)| current_clock >= timer.clock + timer.interval as u32)
            .map(|(id, _)| *id)
            .collect();
        // Timer 1 (interval 5, clock 0): 5 >= 0+5 = true
        // Timer 2 (interval 10, clock 0): 5 >= 0+10 = false
        // Timer 3 (interval 3, clock 0): 5 >= 0+3 = true
        assert!(ready.contains(&1));
        assert!(!ready.contains(&2));
        assert!(ready.contains(&3));
    }

    #[test]
    fn soft_timer_independent_iteration() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 5, 0, None);
        t.add(TimerPriority::Soft, 2, 5, 0, None);

        // Clear normal timers shouldn't affect soft
        t.normal.clear();
        assert_eq!(t.soft.len(), 1);
    }

    #[test]
    fn timer_replace_resets_clock() {
        let mut t = ScriptTimer::new();
        t.add(TimerPriority::Normal, 1, 10, 50, None);
        t.add(TimerPriority::Normal, 1, 10, 100, None); // re-add resets clock
        assert_eq!(t.normal.get(&1).unwrap().clock, 100);
    }
}
