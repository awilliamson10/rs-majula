//! Per-head legality mask (Task 9) -- tells the policy which of the six
//! [`crate::action::MultiAction`] heads are currently legal for a given
//! player, so training/inference can mask out actions that would be no-ops
//! (or outright rejected) by the engine this tick.
//!
//! The observation *vector* (numeric features fed to the policy) is a
//! separate concern, added in Task 10 -- this module currently covers only
//! the mask half.

/// Per-head legality for one player, as of the current tick. `true` means
/// the corresponding [`crate::action::MultiAction`] field is expected to
/// have its intended effect if set this tick (not a guarantee -- e.g.
/// `attack_ok` doesn't check range/cooldown, just that the player exists).
#[derive(Debug, Clone, Copy)]
pub struct Mask {
    /// Always `true` -- movement has no precondition modeled here (pathing/
    /// collision failures are a per-destination runtime concern, not a
    /// per-tick legality one).
    pub move_ok: bool,
    /// The player exists. (Keeping it simple per the task brief -- not
    /// additionally gated on an opponent being present/in-range.)
    pub attack_ok: bool,
    /// Prayer points > 0 (`player.stats.levels[5]`, index 5 = Prayer).
    pub prayer_ok: bool,
    /// Has at least one edible item in the backpack (`first_edible_ro`).
    pub eat_ok: bool,
    /// Has at least one wieldable weapon in the backpack (`first_wieldable_ro`).
    pub equip_ok: bool,
    /// Special-attack energy (`sa_energy`, 0..1000 scale) is at or above
    /// the DDS spec cost (250 -- see `spec_energy`'s doc comment).
    pub spec_ok: bool,
}
