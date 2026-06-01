use crate::active_player::{ActivePlayer, EnginePlayer};
use crate::clients::client_game::create_io;
use crate::engine::{cache, engine, engine_mut};
use crate::handlers::ClientGameHandler;
use core::str::Split;
use num_enum::TryFromPrimitive;
use rs_crypto::isaac::IsaacPair;
use rs_entity::StaffModLevel;
use rs_grid::CoordGrid;
use rs_pack::cache::category::CategoryType;
use rs_pack::cache::r#enum::EnumType;
use rs_pack::cache::idk::IdkType;
use rs_pack::cache::r#if::IfType;
use rs_pack::cache::inv::InvType;
use rs_pack::cache::loc::LocType;
use rs_pack::cache::npc::NpcType;
use rs_pack::cache::obj::ObjType;
use rs_pack::cache::provider::CacheType;
use rs_pack::cache::seq::SeqType;
use rs_pack::cache::spotanim::SpotAnimType;
use rs_pack::cache::r#struct::StructType;
use rs_pack::cache::varp::VarPlayerType;
use rs_pack::cache::{ScriptVarType, VarValue};
use rs_pack::types::{NpcStat, PlayerStat};
use rs_protocol::network::game::client::client_cheat::ClientCheat;
use rs_vm::ScriptError;
use rs_vm::engine::ScriptPlayer;
use rs_vm::state::ScriptArgument;
use rs_vm::subject::ScriptSubject;
use std::panic;
use tracing::error;

/// Handles the `ClientCheat` client protocol message.
///
/// Processes developer/admin cheat commands entered via the client console.
/// The cheat string is validated for length (max 80 characters), parsed into
/// a command name and arguments, then dispatched based on the player's staff
/// moderation level. Only players with `StaffModLevel::Developer` can execute
/// cheat commands.
///
/// # Arguments
///
/// * `active` - The active player whose client sent this message.
///
/// # Returns
///
/// * `Ok(())` on success or if the player lacks permissions.
/// * `Err(ScriptError)` if the cheat string is too long, empty, or a command
///   execution error occurs.
///
/// # Side Effects
///
/// * Dispatches to [`cheat_developer`] for developer-level commands.
///
/// # Call Stack
///
/// **Called by:** `ActivePlayer::decode_and_handle` (via `ClientGameHandler` dispatch)
/// **Calls:** [`cheat_developer`] for developer-level staff
impl ClientGameHandler for ClientCheat {
    fn handle(self, active: &mut ActivePlayer) -> Result<(), ScriptError> {
        if self.cheat.len() > 80 {
            return Err(ScriptError::Client(format!(
                "Client cheat command was too long: {}",
                self.cheat.len()
            )));
        }

        let input_lower = self.cheat.to_lowercase();
        let mut args = input_lower.split(' ');
        let Some(cmd) = args.next() else {
            return Err(ScriptError::Client(format!(
                "Client cheat command was not found for input: {}",
                self.cheat
            )));
        };
        if cmd.is_empty() {
            return Err(ScriptError::Client(format!(
                "Client cheat command was not found for input: {}",
                self.cheat
            )));
        }

        match active.player.staff_mod_level {
            StaffModLevel::Normal => {}
            StaffModLevel::PlayerModerator => {}
            StaffModLevel::JagexModerator => {}
            StaffModLevel::Developer => cheat_developer(cmd, args, active)?,
        }

        Ok(())
    }
}

/// Dispatches and executes developer-level cheat commands.
///
/// Supported commands include:
/// - `~<name>` - Runs a `[debugproc,<name>]` script with typed parameters parsed
///   from the remaining arguments.
/// - `reload` - Sends a signal to reload the engine's script/cache data.
/// - `give <obj> [count]` - Adds an object to the player's inventory.
/// - `setvar <varp> <value>` - Sets a player variable, optionally closing modals
///   and clearing interactions if the variable is protected.
/// - `speed <ms>` - Changes the engine clock rate.
/// - `bots` - Spawns up to 2000 bot players in a grid around a fixed coordinate.
/// - `pickup` - Removes all ground objects within 1 tile of the player.
///
/// # Arguments
///
/// * `cmd` - The lowercased command name (first token of the cheat input).
/// * `args` - An iterator over the remaining space-separated argument tokens.
/// * `active` - The active player executing the cheat command.
///
/// # Returns
///
/// * `Ok(())` on success or for unrecognized commands.
/// * `Err(ScriptError)` if argument parsing fails or a script lookup/execution error occurs.
///
/// # Side Effects
///
/// * May modify engine state (reload, clock rate, spawning bots).
/// * May modify player inventory, variables, or map objects.
///
/// # Call Stack
///
/// **Called by:** `ClientCheat::handle`
/// **Calls:** `engine_mut().run_script_by_name`, `parse_*` helpers, [`cheat_spawn_bots`]
fn cheat_developer(
    cmd: &str,
    mut args: Split<'_, char>,
    active: &mut ActivePlayer,
) -> Result<(), ScriptError> {
    match cmd {
        _ if cmd.starts_with("~") => {
            let name = format!("[debugproc,{}]", &cmd[1..]);
            let Some(script) = engine().scripts.get_by_name(&name).cloned() else {
                return Err(ScriptError::ScriptNotFoundName(name));
            };

            let mut params = Vec::with_capacity(script.info.params.len());

            for index in 0..script.info.params.len() {
                let var_type = ScriptVarType::try_from_primitive(script.info.params[index])
                    .map_err(|_| {
                        ScriptError::Client(format!(
                            "Invalid script var type: {}",
                            script.info.params[index]
                        ))
                    })?;

                match var_type {
                    ScriptVarType::Int => {
                        let value = args.next().unwrap_or("0");
                        params.push(ScriptArgument::Int(value.parse::<i32>().unwrap_or(0)));
                    }
                    ScriptVarType::String => {
                        params.push(ScriptArgument::String(args.next().unwrap_or("").into()))
                    }
                    ScriptVarType::Enum => parse_enum(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Obj | ScriptVarType::NamedObj => parse_obj(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Loc => parse_loc(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Struct => parse_struct(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Boolean => {
                        parse_bool(args.next(), |v| params.push(ScriptArgument::Int(v as i32)))?
                    }
                    ScriptVarType::Coord => parse_coord(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.packed() as i32))
                    })?,
                    ScriptVarType::Category => parse_category(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Spotanim => parse_spotanim(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Npc => parse_npc(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Inv => parse_inv(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Seq => parse_seq(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::Stat => {
                        parse_stat(args.next(), |v| params.push(ScriptArgument::Int(v as i32)))?
                    }
                    ScriptVarType::Interface => parse_interface(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::NpcStat => {
                        parse_npcstat(args.next(), |v| params.push(ScriptArgument::Int(v as i32)))?
                    }
                    ScriptVarType::Idkit => parse_idk(args.next(), |v| {
                        params.push(ScriptArgument::Int(v.id as i32))
                    })?,
                    ScriptVarType::AutoInt => {}
                    ScriptVarType::Component => {}
                    ScriptVarType::Synth => {}
                    ScriptVarType::Varp => {}
                    ScriptVarType::PlayerUid => {}
                    ScriptVarType::NpcUid => {}
                    ScriptVarType::DbRow => {}
                }
            }

            engine_mut().run_script_by_name(
                &name,
                Some(ScriptSubject::Player(active.player.uid)),
                None,
                None,
                None,
                Some(params.into_iter().collect::<Vec<ScriptArgument>>()),
            )
        }
        "reload" => {
            let _ = engine_mut().reload_tx.send(());
            Ok(())
        }
        "give" => {
            let obj_name = args.next();
            let obj = cache()
                .objs
                .get_by_debugname(obj_name.unwrap_or_default())
                .ok_or(ScriptError::ObjNotFoundName(
                    obj_name.unwrap_or_default().into(),
                ))?;
            let inv = cache()
                .invs
                .get_by_debugname("inv")
                .ok_or(ScriptError::InvNotFoundName("inv".into()))?;
            if let Some(inventory) = active.player.invs.get_mut(&inv.id) {
                let count = args.next().unwrap_or("1").parse::<i32>().unwrap_or(1);
                inventory.add(obj.id, count as u32, obj.stackable);
            }
            Ok(())
        }
        "setvar" => parse_varp(args.next(), |v| {
            if v.protect {
                if let Err(e) = active.close_modal(true) {
                    error!("error closing modal during setvar: {e}");
                }
                if !active.can_access() {
                    active.message_game("Please finish what you are doing first.");
                    return;
                }
                active.player.clear_interaction();
                active.unset_map_flag();
            }
            let value = if v.var_type == ScriptVarType::String {
                VarValue::String(args.next().unwrap_or("0").into())
            } else {
                VarValue::from_int(
                    v.var_type,
                    args.next().unwrap_or("0").parse::<i32>().unwrap_or(0),
                )
            };
            active.message_game(&format!("Set {:?}: to {:?}", v.debugname(), value));
            active.set_varp(v.id, value, v.transmit);
        }),
        "speed" => {
            if let Ok(speed) = args.next().unwrap_or("600").parse::<u64>() {
                engine().set_clock_rate(speed);
                active.message_game(&format!("Engine clock rate changed to: {}ms", speed));
            }
            Ok(())
        }
        "bots" => cheat_spawn_bots(active),
        "pickup" => {
            let coord = active.player.pathing.coord;
            let user37 = active.uid().username37();
            let zone = engine_mut().zones.zone_mut(coord.x(), coord.y(), coord.z());
            let objs: Vec<(CoordGrid, u16, u64)> = zone
                .objs
                .iter()
                .filter(|o| {
                    o.coord().in_distance(coord, 1)
                        && (o.receiver37 == rs_entity::obj::NO_RECEIVER || o.receiver37 == user37)
                })
                .map(|o| (o.coord(), o.id(), o.receiver37))
                .collect();
            let count = objs.len();
            for (obj_coord, id, receiver37) in objs {
                let r = if receiver37 == rs_entity::obj::NO_RECEIVER {
                    None
                } else {
                    Some(receiver37)
                };
                engine_mut().remove_obj(obj_coord, id, r, 100);
            }
            active.message_game(&format!("Picked up {} objs.", count));
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Spawns up to 2000 bot players in a 45x45 grid around the coordinate (3222, 0, 3222).
///
/// Each bot is created with a unique PID, dummy ISAAC cipher pair, default
/// walk/run/turn animations, and is immediately teleported to its grid position.
///
/// # Arguments
///
/// * `_player` - The active player who initiated the command (unused).
///
/// # Returns
///
/// * `Ok(())` after spawning as many bots as possible (up to 2000 or until PIDs are exhausted).
///
/// # Side Effects
///
/// * Creates new `ActivePlayer` instances and adds them to the engine's player list.
/// * Logs the number of spawned bots via `tracing::info`.
///
/// # Call Stack
///
/// **Called by:** [`cheat_developer`]
/// **Calls:** `Engine::next_free_pid`, `ActivePlayer::new`, `engine_mut().add_player`
fn cheat_spawn_bots(_player: &ActivePlayer) -> Result<(), ScriptError> {
    let center_x: i32 = 3222;
    let center_z: i32 = 3222;
    let grid_size: i32 = 45; // 45x45 grid = 2025 slots, enough for 2000
    let half: i32 = grid_size / 2;

    let engine = engine_mut();

    let mut spawned = 0;
    for dx in -half..=half {
        for dz in -half..=half {
            if spawned >= 2000 {
                break;
            }

            let x = (center_x + dx) as u16;
            let z = (center_z + dz) as u16;

            let Some(pid) = engine.player_list.next_pid() else {
                break;
            };

            let isaac = IsaacPair::new(&[0; 4], &[0; 4]);
            let io = create_io(isaac);
            let username: Box<str> = format!("Bot {}", pid).into();
            let mut bot = ActivePlayer::new(io.handle, pid, username, false, true);

            bot.player.pathing.coord = CoordGrid::new(x, 0, z);
            bot.player.pathing.last_coord = bot.player.pathing.coord;
            bot.player.pathing.tele = true;
            bot.player.pathing.jump = true;

            bot.player.info.readyanim = Some(808);
            bot.player.info.turnanim = Some(823);
            bot.player.info.walkanim = Some(819);
            bot.player.info.walkanim_b = Some(820);
            bot.player.info.walkanim_l = Some(821);
            bot.player.info.walkanim_r = Some(822);
            bot.player.info.runanim = Some(824);

            bot.buildappearance(0);

            engine.add_player(pid, bot, pid as i64);
            spawned += 1;
        }
        if spawned >= 2000 {
            break;
        }
    }

    tracing::info!(
        "Spawned {} bots around ({}, {}, 0)",
        spawned,
        center_x,
        center_z
    );
    Ok(())
}

/// Looks up an [`ObjType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `ObjType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the object was found and the callback executed.
/// * `Err(ScriptError::ObjNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_obj<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&ObjType),
{
    if let Some(debugname) = value
        && let Some(obj) = cache().objs.get_by_debugname(debugname)
    {
        callback(obj);
        Ok(())
    } else {
        Err(ScriptError::ObjNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up an [`EnumType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `EnumType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the enum was found and the callback executed.
/// * `Err(ScriptError::EnumNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_enum<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&EnumType),
{
    if let Some(debugname) = value
        && let Some(e) = cache().enums.get_by_debugname(debugname)
    {
        callback(e);
        Ok(())
    } else {
        Err(ScriptError::EnumNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up a [`LocType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `LocType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the location type was found and the callback executed.
/// * `Err(ScriptError::LocNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_loc<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&LocType),
{
    if let Some(debugname) = value
        && let Some(loc) = cache().locs.get_by_debugname(debugname)
    {
        callback(loc);
        Ok(())
    } else {
        Err(ScriptError::LocNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up an [`IfType`] (interface type) by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `IfType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the interface was found and the callback executed.
/// * `Err(ScriptError::InterfaceNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_interface<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&IfType),
{
    if let Some(debugname) = value
        && let Some(interface) = cache().interfaces.get_by_debugname(debugname)
    {
        callback(interface);
        Ok(())
    } else {
        Err(ScriptError::InterfaceNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up a [`SpotAnimType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `SpotAnimType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the spot animation was found and the callback executed.
/// * `Err(ScriptError::SpotanimNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_spotanim<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&SpotAnimType),
{
    if let Some(debugname) = value
        && let Some(spotanim) = cache().spotanims.get_by_debugname(debugname)
    {
        callback(spotanim);
        Ok(())
    } else {
        Err(ScriptError::SpotanimNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up an [`NpcType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `NpcType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the NPC type was found and the callback executed.
/// * `Err(ScriptError::NpcNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_npc<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&NpcType),
{
    if let Some(debugname) = value
        && let Some(npc) = cache().npcs.get_by_debugname(debugname)
    {
        callback(npc);
        Ok(())
    } else {
        Err(ScriptError::NpcNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up an [`InvType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `InvType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the inventory type was found and the callback executed.
/// * `Err(ScriptError::InvNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_inv<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&InvType),
{
    if let Some(debugname) = value
        && let Some(inv) = cache().invs.get_by_debugname(debugname)
    {
        callback(inv);
        Ok(())
    } else {
        Err(ScriptError::InvNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up a [`SeqType`] (animation sequence) by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `SeqType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the sequence was found and the callback executed.
/// * `Err(ScriptError::SeqNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_seq<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&SeqType),
{
    if let Some(debugname) = value
        && let Some(seq) = cache().seqs.get_by_debugname(debugname)
    {
        callback(seq);
        Ok(())
    } else {
        Err(ScriptError::SeqNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up an [`IdkType`] (identity kit type) by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `IdkType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the identity kit was found and the callback executed.
/// * `Err(ScriptError::IdkNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_idk<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&IdkType),
{
    if let Some(debugname) = value
        && let Some(idk) = cache().idks.get_by_debugname(debugname)
    {
        callback(idk);
        Ok(())
    } else {
        Err(ScriptError::IdkNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up a [`VarPlayerType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `VarPlayerType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the player variable type was found and the callback executed.
/// * `Err(ScriptError::VarpNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_varp<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&VarPlayerType),
{
    if let Some(debugname) = value
        && let Some(idk) = cache().varps.get_by_debugname(debugname)
    {
        callback(idk);
        Ok(())
    } else {
        Err(ScriptError::VarpNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up a [`StructType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `StructType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the struct type was found and the callback executed.
/// * `Err(ScriptError::StructNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_struct<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&StructType),
{
    if let Some(debugname) = value
        && let Some(s) = cache().structs.get_by_debugname(debugname)
    {
        callback(s);
        Ok(())
    } else {
        Err(ScriptError::StructNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Looks up a [`CategoryType`] by its debug name and invokes the callback with it.
///
/// # Arguments
///
/// * `value` - The debug name string to look up, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `CategoryType` reference on success.
///
/// # Returns
///
/// * `Ok(())` if the category was found and the callback executed.
/// * `Err(ScriptError::CategoryNotFoundName)` if the debug name is missing or not found in the cache.
fn parse_category<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(&CategoryType),
{
    if let Some(debugname) = value
        && let Some(category) = cache().categories.get_by_debugname(debugname)
    {
        callback(category);
        Ok(())
    } else {
        Err(ScriptError::CategoryNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Parses a boolean value from a string and invokes the callback with the result.
///
/// Accepts `"true"`, `"1"`, `"yes"` as truthy and `"false"`, `"0"`, `"no"` as falsy.
///
/// # Arguments
///
/// * `value` - The string to parse, or `None` if no argument was provided.
/// * `callback` - Invoked with the parsed boolean value on success.
///
/// # Returns
///
/// * `Ok(())` if the value was successfully parsed as a boolean.
/// * `Err(ScriptError::Client)` if the value is not a recognized boolean string.
/// * `Err(ScriptError::BooleanNotFoundName)` if no value was provided.
fn parse_bool<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(bool),
{
    if let Some(value) = value {
        if value != "true"
            && value != "false"
            && value != "0"
            && value != "1"
            && value != "yes"
            && value != "no"
        {
            return Err(ScriptError::Client(format!("Invalid boolean: {}", value)));
        }
        callback(value == "yes" || value == "true" || value == "1");
        Ok(())
    } else {
        Err(ScriptError::BooleanNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Parses a coordinate string in `y_mx_mz_lx_lz` format and invokes the callback
/// with the resulting [`CoordGrid`].
///
/// The format uses underscore-separated components:
/// - `y` - Height level (0-3)
/// - `mx` - Map square X (0-255)
/// - `mz` - Map square Z (0-255)
/// - `lx` - Local X within map square (0-63)
/// - `lz` - Local Z within map square (0-63)
///
/// # Arguments
///
/// * `value` - The coordinate string to parse, or `None` if no argument was provided.
/// * `callback` - Invoked with the constructed `CoordGrid` on success.
///
/// # Returns
///
/// * `Ok(())` if the coordinate was successfully parsed and constructed.
/// * `Err(ScriptError::Client)` if the format is invalid or values are out of range.
/// * `Err(ScriptError::IdkNotFoundName)` if no value was provided.
fn parse_coord<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(CoordGrid),
{
    if let Some(coord) = value {
        let parts: Vec<&str> = coord.split('_').collect();
        if parts.len() != 5 {
            return Err(ScriptError::Client(format!(
                "Cannot parse coord: {}",
                coord
            )));
        }
        let y: i32 = parts[0]
            .parse()
            .map_err(|_| ScriptError::Client(format!("Cannot parse coord: {}", coord)))?;
        let mx: i32 = parts[1]
            .parse()
            .map_err(|_| ScriptError::Client(format!("Cannot parse coord: {}", coord)))?;
        let mz: i32 = parts[2]
            .parse()
            .map_err(|_| ScriptError::Client(format!("Cannot parse coord: {}", coord)))?;
        let lx: i32 = parts[3]
            .parse()
            .map_err(|_| ScriptError::Client(format!("Cannot parse coord: {}", coord)))?;
        let lz: i32 = parts[4]
            .parse()
            .map_err(|_| ScriptError::Client(format!("Cannot parse coord: {}", coord)))?;

        if lz < 0 || lx < 0 || mz < 0 || mx < 0 || y < 0 {
            return Err(ScriptError::Client(format!(
                "Cannot parse coord: {}",
                coord
            )));
        }
        if lz > 63 || lx > 63 || mz > 255 || mx > 255 || y > 3 {
            return Err(ScriptError::Client(format!(
                "Cannot parse coord: {}",
                coord
            )));
        }

        let x = (mx << 6) + lx;
        let z = (mz << 6) + lz;
        callback(CoordGrid::from((z | (x << 14) | (y << 28)) as u32));
        Ok(())
    } else {
        Err(ScriptError::IdkNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Parses a player stat name string into a [`PlayerStat`] and invokes the callback with it.
///
/// Uses `PlayerStat::from_config_str` with panic-catching to safely handle invalid names.
///
/// # Arguments
///
/// * `value` - The stat config string to parse, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `PlayerStat` on success.
///
/// # Returns
///
/// * `Ok(())` if the stat was successfully parsed.
/// * `Err(ScriptError::StatNotFoundName)` if the name is invalid or not provided.
fn parse_stat<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(PlayerStat),
{
    if let Some(stat) = value {
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            PlayerStat::from_config_str(stat)
        }));

        match result {
            Ok(stat) => {
                callback(stat);
                Ok(())
            }
            Err(_) => Err(ScriptError::StatNotFoundName(
                value.unwrap_or_default().into(),
            )),
        }
    } else {
        Err(ScriptError::StatNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}

/// Parses an NPC stat name string into an [`NpcStat`] and invokes the callback with it.
///
/// Uses `NpcStat::from_config_str` with panic-catching to safely handle invalid names.
///
/// # Arguments
///
/// * `value` - The NPC stat config string to parse, or `None` if no argument was provided.
/// * `callback` - Invoked with the resolved `NpcStat` on success.
///
/// # Returns
///
/// * `Ok(())` if the stat was successfully parsed.
/// * `Err(ScriptError::NpcstatNotFoundName)` if the name is invalid or not provided.
fn parse_npcstat<F>(value: Option<&str>, callback: F) -> Result<(), ScriptError>
where
    F: FnOnce(NpcStat),
{
    if let Some(stat) = value {
        let result =
            panic::catch_unwind(panic::AssertUnwindSafe(|| NpcStat::from_config_str(stat)));

        match result {
            Ok(stat) => {
                callback(stat);
                Ok(())
            }
            Err(_) => Err(ScriptError::NpcstatNotFoundName(
                value.unwrap_or_default().into(),
            )),
        }
    } else {
        Err(ScriptError::NpcstatNotFoundName(
            value.unwrap_or_default().into(),
        ))
    }
}
