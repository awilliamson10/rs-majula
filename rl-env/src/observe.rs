//! Per-head legality mask (Task 9) -- tells the policy which of the six
//! [`crate::action::MultiAction`] heads are currently legal for a given
//! player, so training/inference can mask out actions that would be no-ops
//! (or outright rejected) by the engine this tick.
//!
//! Also defines the fixed-length partial-info observation vector (Task 10)
//! -- the index map ([`IDX_SELF_HP`] etc.) and [`OBS_LEN`]/[`OPP_HP_BUCKETS`]
//! consumed by [`crate::EnvHarness::observe`].

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
    /// Has at least one edible item in the backpack (`first_edible`).
    pub eat_ok: bool,
    /// Has at least one wieldable weapon in the backpack (`first_wieldable`).
    pub equip_ok: bool,
    /// Special-attack energy (`sa_energy`, 0..1000 scale) is at or above
    /// the DDS spec cost (250 -- see `spec_energy`'s doc comment).
    pub spec_ok: bool,
}

/// Fixed-length partial-info observation vector -- the policy's view of the
/// world. **Self fields are exact** (the client always knows its own state).
/// **Opponent fields are client-visible only**: no exact opponent HP, spec
/// energy, or inventory. In particular, opponent HP is a coarse bucket
/// (`IDX_OPP_HP_BUCKET`, resolution [`OPP_HP_BUCKETS`]) mirroring the RS
/// client's HP bar, never the raw HP number -- see
/// [`crate::EnvHarness::observe`] for the fill logic.
// --- index map (keep in sync with build order in EnvHarness::observe) ---
pub const IDX_SELF_HP: usize = 0;
pub const IDX_SELF_PRAYER: usize = 1;
pub const IDX_SELF_SPEC: usize = 2;
pub const IDX_SELF_RUN: usize = 3;
pub const IDX_SELF_OVERHEAD: usize = 4;
/// Ticks until the next attack is allowed (0 = ready now). Sourced from the
/// `action_delay` varp; see `action::attack_cooldown`.
pub const IDX_SELF_ATKCD: usize = 5;
/// Ticks until eating is allowed again (0 = ready now). Sourced from the
/// `eat_delay` varp; see `action::eat_cooldown`.
pub const IDX_SELF_EATDELAY: usize = 6;
pub const IDX_OPP_DX: usize = 7;
pub const IDX_OPP_DZ: usize = 8;
pub const IDX_OPP_DIST: usize = 9;
/// TODO M1: opponent is-attacking not yet sourced -- left 0.0.
pub const IDX_OPP_ISATTACKING: usize = 10;
pub const IDX_OPP_OVERHEAD: usize = 11;
/// TODO M1: opponent weapon class not yet sourced -- left 0.0.
pub const IDX_OPP_WEAPON: usize = 12;
/// Coarse HP-bar bucket in `[0, OPP_HP_BUCKETS]`, never the exact HP.
pub const IDX_OPP_HP_BUCKET: usize = 13;
/// `1.0` if the opponent was hit during the just-completed cycle, else
/// `0.0`. Sourced from `Player::last_hit_tick` (a plain overwrite, distinct
/// from the `hits` accumulator `step_reward` drains) -- see
/// [`crate::EnvHarness::observe`]'s fill-logic comment for why `hits`
/// itself can't be used here.
pub const IDX_OPP_RECENT_HIT: usize = 14;
/// TODO M1: opponent is-moving not yet sourced -- left 0.0.
pub const IDX_OPP_ISMOVING: usize = 15;
pub const OBS_LEN: usize = 16;

/// Client HP bar resolution (coarse) -- the number of discrete buckets the
/// opponent's HP fraction is quantized into. Never expose the raw opponent
/// HP number in the observation vector; only this bucket index.
pub const OPP_HP_BUCKETS: u8 = 10;
