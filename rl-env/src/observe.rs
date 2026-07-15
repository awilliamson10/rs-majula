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
/// `1.0` if the opponent has an attack cooldown pending (they just swung),
/// else `0.0`. Client-visible: a real player sees the attack animation.
/// Sourced from `action::attack_cooldown`.
pub const IDX_OPP_ISATTACKING: usize = 10;
pub const IDX_OPP_OVERHEAD: usize = 11;
/// Normalized weapon-category code in `[0.0, 1.0]` (`0.0` = unarmed /
/// unknown) for whatever the opponent is wielding. Client-visible: a real
/// player sees the opponent's weapon. Sourced from `action::weapon_class`.
pub const IDX_OPP_WEAPON: usize = 12;
/// Coarse HP-bar bucket in `[0, OPP_HP_BUCKETS]`, never the exact HP.
pub const IDX_OPP_HP_BUCKET: usize = 13;
/// `1.0` if the opponent was hit during the just-completed cycle, else
/// `0.0`. Sourced from `Player::last_hit_tick` (a plain overwrite, distinct
/// from the `hits` accumulator `step_reward` drains) -- see
/// [`crate::EnvHarness::observe`]'s fill-logic comment for why `hits`
/// itself can't be used here.
pub const IDX_OPP_RECENT_HIT: usize = 14;
/// `1.0` if the opponent's tile changed during the just-completed tick,
/// else `0.0`. Client-visible: a real player sees the opponent step.
/// Sourced from [`crate::EnvHarness::note_positions`]'s previous-tick
/// coordinate snapshot -- see [`crate::EnvHarness::note_positions`]'s doc
/// comment for the required call order relative to `observe`.
pub const IDX_OPP_ISMOVING: usize = 15;
/// Soft probability that a DDS special attack, fired NOW, would kill the
/// opponent. Computed ONLY from client-visible state: our own `com_maxhit`,
/// the opponent's COARSE HP bucket (the HP bar), and their overhead prayer
/// icon. Content mechanics: the DDS spec lands TWO hits, each
/// `scale(115,100,%com_maxhit)` (x1.15); protect-from-melee applies
/// `scale(6,10,maxhit)` (x0.6). This is the central PK decision.
pub const IDX_SPEC_KO_CHANCE: usize = 16;
/// Magnitude of the last damage WE dealt (normalized by /40). Survives
/// `step_reward_pair`'s drain of the `hits` accumulator.
pub const IDX_LAST_DEALT: usize = 17;
/// Magnitude of the last damage WE took (normalized by /40).
pub const IDX_LAST_TAKEN: usize = 18;
/// Edible items remaining in the backpack, normalized by /28 (inventory size).
pub const IDX_FOOD_REMAINING: usize = 19;
pub const OBS_LEN: usize = 20;

/// Client HP bar resolution (coarse) -- the number of discrete buckets the
/// opponent's HP fraction is quantized into. Never expose the raw opponent
/// HP number in the observation vector; only this bucket index.
pub const OPP_HP_BUCKETS: u8 = 10;
