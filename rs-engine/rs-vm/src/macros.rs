use crate::ScriptError;
use crate::state::{LocRef, ObjRef, ScriptState};

/// Extracts the player index (`pid`) from the current script state's active player.
///
/// Reads the int operand from the script state to determine whether to use the
/// primary or secondary active player pointer.
///
/// # Arguments
/// * `state` - The current script execution state.
///
/// # Returns
/// The `pid` (player index) of the active player on success.
///
/// # Panics / Errors
/// Returns a `ScriptError::Runtime` if no active player is set for the chosen slot.
///
/// # Call Stack
/// **Called by:** `active_player!`, `active_player_mut!`, `protected_active_player!`,
/// `protected_active_player_mut!` macros.
/// **Calls:** [`ScriptState::int_operand`], [`PlayerUid::pid`]
pub(crate) fn active_player_pid(state: &ScriptState) -> crate::Result<u16> {
    let secondary = state.int_operand() != 0;
    let uid = if secondary {
        state.active_player2
    } else {
        state.active_player
    }
    .ok_or_else(|| ScriptError::Runtime("no active_player".into()))?;
    Ok(uid.pid())
}

/// Extracts the NPC index (`nid`) from the current script state's active NPC.
///
/// Reads the int operand from the script state to determine whether to use the
/// primary or secondary active NPC pointer.
///
/// # Arguments
/// * `state` - The current script execution state.
///
/// # Returns
/// The `nid` (NPC index) of the active NPC on success.
///
/// # Panics / Errors
/// Returns a `ScriptError::Runtime` if no active NPC is set for the chosen slot.
///
/// # Call Stack
/// **Called by:** `active_npc!`, `active_npc_mut!` macros.
/// **Calls:** [`ScriptState::int_operand`], [`NpcUid::nid`]
pub(crate) fn active_npc_nid(state: &ScriptState) -> crate::Result<u16> {
    let secondary = state.int_operand() != 0;
    let uid = if secondary {
        state.active_npc2
    } else {
        state.active_npc
    }
    .ok_or_else(|| ScriptError::Runtime("no active_npc".into()))?;
    Ok(uid.nid())
}

/// Extracts the [`LocRef`] from the current script state's active location.
///
/// Reads the int operand from the script state to determine whether to use the
/// primary or secondary active location pointer.
///
/// # Arguments
/// * `state` - The current script execution state.
///
/// # Returns
/// The [`LocRef`] of the active location on success.
///
/// # Panics / Errors
/// Returns a `ScriptError::Runtime` if no active location is set for the chosen slot.
///
/// # Call Stack
/// **Called by:** `active_loc!`, `active_loc_mut!` macros.
/// **Calls:** [`ScriptState::int_operand`]
pub(crate) fn active_loc_ref(state: &ScriptState) -> crate::Result<LocRef> {
    let secondary = state.int_operand() != 0;
    if secondary {
        state.active_loc2
    } else {
        state.active_loc
    }
    .ok_or_else(|| ScriptError::Runtime("no active_loc".into()))
}

/// Extracts the [`ObjRef`] from the current script state's active ground object.
///
/// Reads the int operand from the script state to determine whether to use the
/// primary or secondary active object pointer.
///
/// # Arguments
/// * `state` - The current script execution state.
///
/// # Returns
/// The [`ObjRef`] of the active ground object on success.
///
/// # Panics / Errors
/// Returns a `ScriptError::Runtime` if no active object is set for the chosen slot.
///
/// # Call Stack
/// **Called by:** `active_obj!`, `active_obj_mut!` macros.
/// **Calls:** [`ScriptState::int_operand`]
pub(crate) fn active_obj_ref(state: &ScriptState) -> crate::Result<ObjRef> {
    let secondary = state.int_operand() != 0;
    if secondary {
        state.active_obj2
    } else {
        state.active_obj
    }
    .ok_or_else(|| ScriptError::Runtime("no active_obj".into()))
}

#[macro_export]
macro_rules! handlers {
    (|$m:ident| $($body:tt)*) => {{
        let mut $m = $crate::register::OpsRegistry::new();
        $($body)*
        $m
    }};
}

#[macro_export]
macro_rules! none {
    ($m:ident, $op:ident => |$s:ident| $body:block) => {
        $m.insert($op, |$s| {
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_player {
    ($m:ident, $op:ident => |$s:ident, $player:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_player($s)?;
            let pid = $crate::macros::active_player_pid($s)?;
            let $player = unsafe { $crate::engine::engine_typed::<E>() }
                .get_player(pid)
                .ok_or_else(|| {
                    $crate::ScriptError::Runtime(format!("active player slot empty: {}", pid))
                })?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_player_mut {
    ($m:ident, $op:ident => |$s:ident, $player:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_player($s)?;
            let pid = $crate::macros::active_player_pid($s)?;
            let $player = unsafe { $crate::engine::engine_typed_mut::<E>() }
                .get_player_mut(pid)
                .ok_or_else(|| {
                    $crate::ScriptError::Runtime(format!("active player slot empty: {}", pid))
                })?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_npc {
    ($m:ident, $op:ident => |$s:ident, $npc:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_npc($s)?;
            let nid = $crate::macros::active_npc_nid($s)?;
            let $npc = unsafe { $crate::engine::engine_typed::<E>() }
                .get_npc(nid)
                .ok_or_else(|| {
                    $crate::ScriptError::Runtime(format!("active npc slot empty: {}", nid))
                })?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_npc_mut {
    ($m:ident, $op:ident => |$s:ident, $npc:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_npc($s)?;
            let nid = $crate::macros::active_npc_nid($s)?;
            let $npc = unsafe { $crate::engine::engine_typed_mut::<E>() }
                .get_npc_mut(nid)
                .ok_or_else(|| {
                    $crate::ScriptError::Runtime(format!("active npc slot empty: {}", nid))
                })?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_loc {
    ($m:ident, $op:ident => |$s:ident, $loc:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_loc($s)?;
            let $loc = $crate::macros::active_loc_ref($s)?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_loc_mut {
    ($m:ident, $op:ident => |$s:ident, $loc:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_loc($s)?;
            let $loc = $crate::macros::active_loc_ref($s)?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_obj {
    ($m:ident, $op:ident => |$s:ident, $obj:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_obj($s)?;
            let $obj = $crate::macros::active_obj_ref($s)?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! active_obj_mut {
    ($m:ident, $op:ident => |$s:ident, $obj:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_active_obj($s)?;
            let $obj = $crate::macros::active_obj_ref($s)?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! protected_active_player {
    ($m:ident, $op:ident => |$s:ident, $player:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_protected_active_player($s)?;
            let pid = $crate::macros::active_player_pid($s)?;
            let $player = unsafe { $crate::engine::engine_typed::<E>() }
                .get_player(pid)
                .ok_or_else(|| {
                    $crate::ScriptError::Runtime(format!(
                        "active protected player slot empty: {}",
                        pid
                    ))
                })?;
            $body;
            Ok(())
        });
    };
}

#[macro_export]
macro_rules! protected_active_player_mut {
    ($m:ident, $op:ident => |$s:ident, $player:ident| $body:block) => {
        $m.insert($op, |$s| {
            $crate::util::require_protected_active_player($s)?;
            let pid = $crate::macros::active_player_pid($s)?;
            let $player = unsafe { $crate::engine::engine_typed_mut::<E>() }
                .get_player_mut(pid)
                .ok_or_else(|| {
                    $crate::ScriptError::Runtime(format!(
                        "active protected player slot empty: {}",
                        pid
                    ))
                })?;
            $body;
            Ok(())
        });
    };
}
