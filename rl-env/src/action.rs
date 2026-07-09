//! RL action space: a single flat [`MultiAction`] carrying one intent per
//! action "head" (move, attack, prayer, eat, equip, spec), applied to a
//! player every tick via [`crate::EnvHarness::apply_actions`].
//!
//! Only `move_*` and `attack` are wired up as of this task (Task 6);
//! `prayer`/`eat`/`equip`/`spec` are carried on the struct but are no-ops
//! until Tasks 7-8 implement their handlers -- callers may set them today
//! without any effect.

use rs_engine::{ActivePlayer, ClientGameHandler};
use rs_protocol::network::game::client::move_gameclick::MoveGameClick;
use rs_protocol::network::game::client::pack_coord;

/// What a player should do w.r.t. combat this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackIntent {
    /// No change to the current combat interaction.
    Hold,
    /// Engage the opponent (equivalent to clicking "Attack" on them).
    Engage,
    /// Clear any active combat interaction (equivalent to walking away /
    /// cancelling the fight).
    Disengage,
}

/// A single tick's worth of action across every action head. Fields the
/// current task doesn't wire up are carried through unused; see the module
/// docs.
#[derive(Debug, Clone, Copy)]
pub struct MultiAction {
    /// Relative destination tile offset, x axis. Window is +/-8; 0 means no
    /// move this tick.
    pub move_dx: i8,
    /// Relative destination tile offset, z axis. Window is +/-8; 0 means no
    /// move this tick.
    pub move_dz: i8,
    pub attack: AttackIntent,
    /// 0 = none, 1 = protect-melee (Task 8). No-op this task.
    pub prayer: u8,
    /// Eat a food item from the inventory (Task 7). No-op this task.
    pub eat: bool,
    /// 0 = no switch, 1.. = gear set index (Task 7). No-op this task.
    pub equip: u8,
    /// Trigger the equipped weapon's special attack (Task 8). No-op this
    /// task.
    pub spec: bool,
}

/// Walks `active` toward `dest` by injecting a real `MoveGameClick` through
/// the same handler the client's game-viewport click would hit -- i.e. this
/// goes through actual pathing/collision, unlike [`crate::EnvHarness::attack_player`]'s
/// direct-interaction-injection shortcut. Must be called with the engine's
/// thread-local state installed (i.e. from inside `with_engine`), since
/// `MoveGameClick::handle` reads `engine()`.
///
/// A single destination waypoint is sufficient: the pathing system steps
/// one (or two, running) tile per tick toward the last-queued waypoint
/// (`rs-entity/src/pathing.rs::try_step`), so re-issuing this call every
/// tick with a freshly-computed relative destination (as
/// [`crate::EnvHarness::apply_actions`] does) reads as continuous movement
/// in that direction.
pub fn move_to(active: &mut ActivePlayer, dest: rs_grid::CoordGrid) {
    let packed = pack_coord(dest.x(), dest.z());
    let msg = MoveGameClick { path: vec![packed], ctrl: false };
    let _ = msg.handle(active);
}
