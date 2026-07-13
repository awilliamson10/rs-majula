//! RL action space: a single flat [`MultiAction`] carrying one intent per
//! action "head" (move, attack, prayer, eat, equip, spec), applied to a
//! player every tick via [`crate::EnvHarness::apply_actions`].
//!
//! All six heads are wired up as of this task (Task 8 adds `prayer`/`spec`
//! on top of Task 7's `move`/`attack`/`eat`/`equip`).

use once_cell::sync::OnceCell;
use rs_engine::{ActivePlayer, ClientGameHandler};
use rs_protocol::network::game::client::if_button::IfButton;
use rs_protocol::network::game::client::move_gameclick::MoveGameClick;
use rs_protocol::network::game::client::opheld1::OpHeld1;
use rs_protocol::network::game::client::opheld2::OpHeld2;
use rs_protocol::network::game::client::opheld3::OpHeld3;
use rs_protocol::network::game::client::opheld4::OpHeld4;
use rs_protocol::network::game::client::opheld5::OpHeld5;
use rs_protocol::network::game::client::pack_coord;

// -- Content spike (Task 8) -------------------------------------------------
//
// Discovered from `content/274` (see doc comments on each item below for the
// exact citations). Two of the four are genuine content constants (baked
// directly from `.constant`/`.varp` files, which are stable data, not pack
// output); the other two are interface *component* ids, which -- like
// `inv_com()` above -- are pack-time-assigned and not safe to hardcode as
// numeric literals, so they are resolved once via `cache().interfaces
// .get_by_debugname(...)` and memoized, exactly like `inv_com()`.

/// `player.headicons` bit for the Protect-from-Melee overhead prayer icon.
///
/// `content/274/scripts/player/configs/headicon.constant:4`:
/// `^headicon_prayer_protectfrommelee = 3` (icon index 3). The bit itself is
/// computed from that index by `[proc,headicon_add]`/`[proc,headicon_del]`
/// (`content/274/scripts/player/scripts/appearance.rs2:64-73`):
/// `def_int $bit = multiply(0x1, pow(2, $icon));` i.e. `bit = 1 << icon`, so
/// icon 3 -> bit `1 << 3 = 8`. Confirmed against the runtime field type via
/// `rs-engine/src/engine.rs:4729` (`fn headicons_get(&self) -> u8`).
pub const HEADICON_PROTECT_MELEE: u8 = 1 << 3;

/// Player varp debugname for special-attack energy (0..1000, hundredths of a
/// percent -- e.g. `1000` = 100%).
///
/// `content/274/scripts/_unpack/244/all.varp:4`: `[sa_energy]`. Used
/// throughout `content/274/scripts/skill_combat/scripts/player/specwep.rs2`
/// as `%sa_energy` (e.g. line 16: `if (%sa_energy = ^sa_max_energy)`, line
/// 19: `%sa_energy = max(sub(%sa_energy, $energy_used), 0);`).
pub const VARP_SPEC_ENERGY: &str = "sa_energy";

/// Interface component id (`IfButton.com`) for the Protect-from-Melee prayer
/// button, resolved via its full `"{interface}:{component}"` debugname.
///
/// The debugname `"prayer:prayer_protectfrommelee"` is exactly the string
/// content itself uses to key this button's trigger:
/// `content/274/scripts/skill_prayer/scripts/prayers/protectfrommelee.rs2:1`:
/// `[if_button,prayer:prayer_protectfrommelee]`. The component is defined in
/// the prayer tab interface at
/// `content/274/scripts/skill_prayer/interfaces/prayer.if:157`:
/// `[prayer_protectfrommelee]` (`buttontype=toggle`). Resolved the same way
/// as [`inv_com`] (`cache().interfaces.get_by_debugname`), not hardcoded,
/// since interface component ids are pack-assigned.
static COM_PROTECT_MELEE_CELL: OnceCell<u16> = OnceCell::new();
pub fn com_protect_melee() -> u16 {
    *COM_PROTECT_MELEE_CELL.get_or_init(|| {
        crate::cache()
            .interfaces
            .get_by_debugname("prayer:prayer_protectfrommelee")
            .expect(
                "cache is missing the \"prayer:prayer_protectfrommelee\" interface \
                 component (see content/274/scripts/skill_prayer/interfaces/prayer.if:157)",
            )
            .id
    })
}

/// Interface component id (`IfButton.com`) for the special-attack bar
/// toggle, resolved via its full `"{interface}:{component}"` debugname.
///
/// Every weapon category has its own combat sub-interface with its own
/// `specbar` component (see `[proc,update_weapon_category]`,
/// `content/274/scripts/skill_combat/scripts/player/player_attackstyles.rs2:102-141`,
/// which switches the active combat tab -- and thus which `specbar` is
/// visible -- on every equip/unequip). This constant targets
/// `"combat_stabsword:specbar"`, the stab-weapon tab's spec bar, i.e. the
/// tab `dragon_dagger` (`category=weapon_stab`) switches to when wielded --
/// matching this env's designated spec weapon (`mirror_melee.ron`). The
/// component is defined at
/// `content/274/scripts/skill_combat/interfaces/melee/combat_stabsword.if:282`
/// (`[specbar]`, `buttontype=normal`), and its trigger at
/// `content/274/scripts/skill_combat/scripts/player/specwep.rs2:85`:
/// `[if_button,combat_stabsword:specbar] @toggle_sa;`. A weapon-category-
/// agnostic spec toggle (resolving the right tab for whatever is currently
/// wielded) is out of scope for this content spike -- see module docs.
static COM_SPECIAL_ATTACK_CELL: OnceCell<u16> = OnceCell::new();
pub fn com_special_attack() -> u16 {
    *COM_SPECIAL_ATTACK_CELL.get_or_init(|| {
        crate::cache()
            .interfaces
            .get_by_debugname("combat_stabsword:specbar")
            .expect(
                "cache is missing the \"combat_stabsword:specbar\" interface \
                 component (see content/274/scripts/skill_combat/interfaces/melee/combat_stabsword.if:282)",
            )
            .id
    })
}

/// Fires an `IfButton` click on component `com` through the real handler
/// (`rs-engine/src/handlers/if_button.rs`), exactly as a client click on
/// that interface component would. Used by [`crate::EnvHarness::apply_actions`]'s
/// `prayer`/`spec` heads (Task 8).
pub fn if_button(active: &mut ActivePlayer, com: u16) {
    let _ = IfButton { com }.handle(active);
}

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

// -- Resolved-action recording (Task 12) ------------------------------------
//
// `MultiAction` is the *requested* intent; `ResolvedKind`/`ResolvedAction`
// record what `EnvHarness::apply_actions` actually dispatched -- e.g. a
// `prayer: 1` request that matches the already-active overhead is a no-op
// (see the toggle guards in `apply_actions`) and is NOT recorded, whereas an
// actual toggle click is. This is the compact log Phase C replays to
// reproduce a fight deterministically, so it must only ever contain events
// that really happened, not every head that was merely requested.

/// One action head's *actual* dispatch this tick (as opposed to
/// [`MultiAction`]'s requested intent) -- see the module note above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedKind {
    Move { dx: i8, dz: i8 },
    Attack,
    Disengage,
    Eat,
    Equip,
    Prayer(u8),
    Spec,
}

/// A single recorded dispatch: which player (`pid`), on which
/// [`crate::EnvHarness`] episode tick, did what ([`ResolvedKind`]). Pushed by
/// [`crate::EnvHarness::apply_actions`], drained by
/// [`crate::EnvHarness::drain_recorded`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedAction {
    pub pid: u16,
    pub tick: u32,
    pub kind: ResolvedKind,
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
    /// 0 = none, 1 = protect-melee (Task 8). Toggles the Protect-from-Melee
    /// overhead prayer on/off via [`if_button`]/[`com_protect_melee`],
    /// checking current state first (`player.headicons &
    /// HEADICON_PROTECT_MELEE`) so holding the same value across ticks
    /// doesn't flip-flop the prayer script.
    pub prayer: u8,
    /// Eat the first edible backpack item (Task 7): ops its "Eat" iop via
    /// `OpHeld{op}`, see [`first_edible`]/[`op_held`].
    pub eat: bool,
    /// 0 = no switch, 1 = wield the first wieldable backpack item (Task 7,
    /// M1's only defined gear-set index -- see [`first_wieldable`]).
    /// Values > 1 are reserved for future gear-set indices and are no-ops.
    pub equip: u8,
    /// Trigger the special attack bar's toggle (Task 8) via
    /// [`if_button`]/[`com_special_attack`] -- see that function's docs for
    /// the current weapon-category caveat.
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

// -- Legality mask support (Task 9) -----------------------------------------
//
// `first_edible`/`first_wieldable` above already take `&ActivePlayer` (not
// `&mut`) -- Task 7 wrote them read-only from the start, since all the
// actual mutation happens in `op_held`'s handler call, not the scan. So
// there is no separate `_ro` variant to add here (the task brief's "add a
// read-only variant if the existing one needs `&mut`" caveat doesn't apply):
// `crate::observe`'s `legal_mask` calls `first_edible`/`first_wieldable`
// directly.

/// Special-attack energy cost of the dragon dagger's special attack, on the
/// same 0..1000 scale as [`spec_energy`]'s return value -- i.e. this is
/// *not* `25` (a 0..100 "percent" reading would suggest that), it's `250`
/// out of `1000`. Sourced from the weapon's own obj param, not a guess:
/// `content/274/scripts/skill_combat/configs/melee/daggers.obj:251,560`:
/// `param=sa_energy,250` (the `oc_param(...,sa_energy)` read by
/// `content/274/scripts/skill_combat/scripts/pvp/specs/scripts/pvp_dragon_dagger.rs2:2`,
/// `~set_sa_vars(oc_param(inv_getobj(worn, ^wearpos_rhand), sa_energy))`,
/// which feeds `[proc,sub_sa_energy]`'s `%sa_energy = max(sub(%sa_energy,
/// $energy_used), 0)` against the same `%sa_energy` varp `spec_energy`
/// reads).
pub const SPEC_COST_DRAGON_DAGGER: i32 = 250;

/// Reads a player's current special-attack energy (the `sa_energy` varp,
/// [`VARP_SPEC_ENERGY`]) -- 0..1000 scale, e.g. `1000` = a full bar. Resolves
/// the varp id via [`crate::cache`] each call rather than memoizing (unlike
/// [`com_protect_melee`]/[`com_special_attack`]'s `OnceCell`s) since a varp
/// id lookup is a cheap map get, not a full interface-tree scan.
pub fn spec_energy(active: &ActivePlayer) -> i32 {
    let varp = crate::cache()
        .varps
        .get_by_debugname(VARP_SPEC_ENERGY)
        .expect("cache is missing the \"sa_energy\" varp");
    active.player.vars.get(varp.id).as_int()
}

// -- Combat-timing observability (Task 1) -----------------------------------

/// Player varp holding the tick at which the next attack becomes allowed.
/// Content (`skill_combat/scripts/pvp/pvp_melee.rs2`) sets
/// `%action_delay = add(map_clock, oc_param($rhand, attackrate))` after each
/// swing, and refuses to attack while `%action_delay > map_clock`. So the
/// ticks-remaining is `max(0, action_delay - clock)`.
pub const VARP_ACTION_DELAY: &str = "action_delay";

/// Player varp holding the tick at which eating becomes allowed again.
/// Content (`player/scripts/consumption/consume.rs2`) sets
/// `%eat_delay = calc(map_clock + $eat_delay)` and blocks while
/// `%eat_delay >= map_clock`.
pub const VARP_EAT_DELAY: &str = "eat_delay";

fn varp_int(active: &ActivePlayer, name: &str) -> i32 {
    let varp = crate::cache()
        .varps
        .get_by_debugname(name)
        .unwrap_or_else(|| panic!("cache is missing the {name:?} varp"));
    active.player.vars.get(varp.id).as_int()
}

/// Ticks until `active` may attack again (0 = ready now). See
/// [`VARP_ACTION_DELAY`]. This is THE combat-timing signal -- an agent that
/// cannot see it can only learn a statistical prior over swing timing.
pub fn attack_cooldown(active: &ActivePlayer, clock: u32) -> u32 {
    let until = varp_int(active, VARP_ACTION_DELAY);
    (until - clock as i32).max(0) as u32
}

/// Ticks until `active` may eat again (0 = ready now). See [`VARP_EAT_DELAY`].
pub fn eat_cooldown(active: &ActivePlayer, clock: u32) -> u32 {
    let until = varp_int(active, VARP_EAT_DELAY);
    (until - clock as i32).max(0) as u32
}

// -- Opponent observations (Task 2) ------------------------------------------

/// Normalized weapon-category code for whatever `active` is wielding, in
/// `[0.0, 1.0]` (0.0 = unarmed / unknown). Client-visible: a real player sees
/// the opponent's weapon. The obj's `category` (e.g. `weapon_slash` for
/// `rune_scimitar`, `weapon_stab` for `dragon_dagger` -- confirmed against
/// `content/274/scripts/skill_combat/configs/melee/*.obj`) is a
/// content-declared param; we map the category id into a bounded float
/// rather than exposing a raw id, so the network sees a stable small-range
/// value.
///
/// Reads specifically the `WearPos::RightHand` slot of the "worn" inv, NOT
/// "the first worn slot with any category": armour also carries a
/// `category` (`rune_full_helm` -> `armour_helmet`, `rune_platebody` ->
/// `armour_body`, etc. -- same config files), so scanning worn slots in
/// order for the first one with *any* category would report whichever
/// armour piece happens to be worn before the weapon slot instead of the
/// weapon itself.
///
/// Normalizes by `cache.categories.count()`, NOT a hardcoded scale: category
/// ids run well past 64 in the 274 cache (`rune_scimitar` -> 197,
/// `dragon_dagger` -> 221, out of 297 total categories), so a fixed
/// `/ 64.0` clamped every armed reading to a constant `1.0` -- a dead
/// feature. In mirror melee both sides start with identical gear, so this
/// value's only signal is "the opponent just swapped weapons" (e.g. to a
/// dragon dagger for a spec); a constant can't carry that. Dividing by the
/// real category count keeps every valid id (which is always `< count`, so
/// a valid weapon's category never clamps) apart.
pub fn weapon_class(active: &ActivePlayer) -> f32 {
    let cache = crate::cache();
    let Some(worn_inv) = cache.invs.get_by_debugname("worn").map(|i| i.id) else { return 0.0 };
    let Some(inv) = active.player.invs.get(&worn_inv) else { return 0.0 };
    let rhand = rs_pack::types::WearPos::RightHand as usize;
    let Some(Some(item)) = inv.slots.get(rhand) else { return 0.0 };
    let Some(obj) = cache.objs.get_by_id(item.obj) else { return 0.0 };
    let Some(category) = obj.category else { return 0.0 };
    let scale = cache.categories.count().max(1) as f32;
    ((category as f32) / scale).clamp(0.0, 1.0)
}
