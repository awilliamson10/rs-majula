use crate::state::{LocRef, ObjRef};
use crate::{NpcUid, PlayerUid};

/// Represents the subject (target entity) of a script execution.
///
/// When a script is triggered, this enum identifies which kind of game entity the
/// script is operating on. The VM uses this to set the appropriate active entity
/// pointers in [`ScriptState`](crate::state::ScriptState) before execution begins.
///
/// # Variants
/// * `Player` - A player entity, identified by its packed [`PlayerUid`].
/// * `Npc` - An NPC entity, identified by its packed [`NpcUid`].
/// * `Loc` - A location (world object/scenery), identified by a [`LocRef`].
/// * `Obj` - A ground item/object, identified by an [`ObjRef`].
pub enum ScriptSubject {
    Player(PlayerUid),
    Npc(NpcUid),
    Loc(LocRef),
    Obj(ObjRef),
}
