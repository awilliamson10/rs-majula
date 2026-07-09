//! RL action space: a single flat [`MultiAction`] carrying one intent per
//! action "head" (move, attack, prayer, eat, equip, spec), applied to a
//! player every tick via [`crate::EnvHarness::apply_actions`].
//!
//! `move_*`, `attack`, `eat`, and `equip` are wired up as of this task
//! (Task 7); `prayer`/`spec` are carried on the struct but remain no-ops
//! until Task 8 implements their handlers -- callers may set them today
//! without any effect.

use once_cell::sync::OnceCell;
use rs_engine::{ActivePlayer, ClientGameHandler};
use rs_protocol::network::game::client::move_gameclick::MoveGameClick;
use rs_protocol::network::game::client::opheld1::OpHeld1;
use rs_protocol::network::game::client::opheld2::OpHeld2;
use rs_protocol::network::game::client::opheld3::OpHeld3;
use rs_protocol::network::game::client::opheld4::OpHeld4;
use rs_protocol::network::game::client::opheld5::OpHeld5;
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
    /// Eat the first edible backpack item (Task 7): ops its "Eat" iop via
    /// `OpHeld{op}`, see [`first_edible`]/[`op_held`].
    pub eat: bool,
    /// 0 = no switch, 1 = wield the first wieldable backpack item (Task 7,
    /// M1's only defined gear-set index -- see [`first_wieldable`]).
    /// Values > 1 are reserved for future gear-set indices and are no-ops.
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

/// Interface component id for the backpack inventory tab
/// (`"inventory:inv"`), i.e. the `com` that `OpHeld1..5`'s shared handler
/// validates the request against.
///
/// `rs-engine/src/handlers/opheld.rs:132-153` requires `com` to resolve to
/// a visible+operable interface (`cache().interfaces.get_by_id(com)`,
/// `active.player.is_interface_visible(interface.root_layer)`,
/// `interface.operable`) *and* to be registered in
/// `active.player.inv_transmits` against the target inv
/// (`inv_transmits.iter().find(|(_, coms)| coms.contains(&com))`). The real
/// client establishes both via the login script's tab-setup proc,
/// `content/274/scripts/login_logout/login.rs2` (`[proc,initalltabs]`):
/// `inv_transmit(inv, inventory:inv); if_settab(inventory, ^tab_inventory);`
/// -- `"inventory:inv"` is the exact identifier that call binds the
/// backpack ("inv") inv to, so it is also the `com` OpHeld needs here.
/// Resolved once (via [`crate::cache`]) rather than hardcoded, since it is
/// content-derived, not a stable literal.
static INV_COM_CELL: OnceCell<u16> = OnceCell::new();
pub fn inv_com() -> u16 {
    *INV_COM_CELL.get_or_init(|| {
        crate::cache()
            .interfaces
            .get_by_debugname("inventory:inv")
            .expect(
                "cache is missing the \"inventory:inv\" interface component \
                 (see rs-engine/src/handlers/opheld.rs `com` validation)",
            )
            .id
    })
}

/// Fires held-op `op` (1-5, i.e. `OpHeld{op}`) on `obj`/`slot` through the
/// real handler (`rs-engine/src/handlers/opheld.rs`), using the backpack's
/// `com` ([`inv_com`]).
///
/// `op` is *not* fixed per action head -- it is the obj's own iop-table
/// index for the desired verb (1-based; see [`first_edible`] /
/// [`first_wieldable`]), because that index varies per obj. Confirmed by
/// direct cache inspection: `shark`'s `"Eat"` is iop index 0 (op 1), but
/// `dragon_dagger`'s `"Wield"` is iop index 1 (op 2) -- `iop[0]` is `None`
/// for `dragon_dagger` (its op 1 is unused/reserved), so calling
/// `OpHeld1` on it would fail `opheld.rs`'s
/// `iop.get(op - 1).is_none_or(|o| o.is_none())` check. An out-of-range
/// `op` is a no-op (nothing calls this with anything but a table-derived
/// 1-5 value).
pub fn op_held(active: &mut ActivePlayer, op: usize, obj: u16, slot: u16, com: u16) {
    let _ = match op {
        1 => OpHeld1 { obj, slot, com }.handle(active),
        2 => OpHeld2 { obj, slot, com }.handle(active),
        3 => OpHeld3 { obj, slot, com }.handle(active),
        4 => OpHeld4 { obj, slot, com }.handle(active),
        5 => OpHeld5 { obj, slot, com }.handle(active),
        _ => return,
    };
}

/// Scans `active`'s backpack ("inv") inventory for the first occupied slot
/// whose obj has an iop entry exactly equal to `verb` (e.g. `"Eat"`,
/// `"Wield"`), returning `(slot, obj_id, op)` where `op` is the 1-based
/// iop index `op_held`/`OpHeld{op}` expects. Returns `None` if the player
/// has no "inv" inventory, or no backpack item currently has that iop.
fn find_first_iop(
    active: &ActivePlayer,
    cache: &rs_pack::cache::CacheStore,
    verb: &str,
) -> Option<(u16, u16, usize)> {
    let inv_id = cache.invs.get_by_debugname("inv")?.id;
    let inv = active.player.invs.get(&inv_id)?;
    for (idx, slot) in inv.slots.iter().enumerate() {
        let Some(item) = slot else { continue };
        let Some(obj) = cache.objs.get_by_id(item.obj) else { continue };
        let Some(iop) = &obj.iop else { continue };
        if let Some(pos) = iop.iter().position(|o| o.as_deref() == Some(verb)) {
            return Some((idx as u16, obj.id, pos + 1));
        }
    }
    None
}

/// First backpack slot holding a food item (obj iop contains `"Eat"`).
/// Returns `(slot, obj_id, op)` for [`op_held`].
pub fn first_edible(active: &ActivePlayer) -> Option<(u16, u16, usize)> {
    find_first_iop(active, crate::cache(), "Eat")
}

/// First backpack slot holding a wieldable weapon (obj iop contains
/// `"Wield"`). Returns `(slot, obj_id, op)` for [`op_held`].
pub fn first_wieldable(active: &ActivePlayer) -> Option<(u16, u16, usize)> {
    find_first_iop(active, crate::cache(), "Wield")
}
