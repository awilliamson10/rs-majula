//! Task 2c: `host_load_profile`'s restore must REPLACE perm state, not MERGE
//! onto it.
//!
//! ★★ ONE ENGINE PER PROCESS, THEREFORE ONE `#[test]` FN IN THIS BINARY.
//! `EnvHarness::boot_seeded` trips the same process-global `rs-pathfinder`
//! collision state `host_new` does, even though this test never calls
//! `host_new` -- see `gen_changed_profile.rs`'s own header for the same
//! reasoning.
//!
//! # ★ WHY THIS GOES THROUGH `rs_engine::player_save` DIRECTLY, NOT THE C ABI
//! `host_load_profile`/`host_save_profile` are the right layer for Task 2's
//! and 2b's crash/resync regressions, but there is no ABI entry point that
//! DIRTIES a live player from outside -- and `ClientCheat` (`::setvar`,
//! `::give`) is gated to `StaffModLevel::Developer`, which defaults to
//! `#[cfg(debug_assertions)]` (`rs-entity/src/player.rs:295-297`) and is
//! FALSE in the `--release` build `HOST_DYLIB` points at, so it would
//! silently no-op there anyway. `gen_changed_profile.rs` establishes the
//! precedent this test follows: build the dirty state directly on a
//! throwaway `EnvHarness`, poke `player.vars`/`player.invs` by hand, and
//! call the exact `rs_engine::player_save` functions `host_load_profile`
//! calls -- proving the fix at the engine layer, which is where the bug
//! actually lives.
use rs_grid::CoordGrid;
use rs_inv::{Inventory, StackMode};
use rs_pack::cache::VarValue;

#[test]
fn restoring_a_default_profile_clears_perm_state_the_profile_never_captured() {
    let mut env = rl_env::EnvHarness::boot_seeded(4242);
    let (pid, _rx) = env
        .engine
        .spawn_player_tapped("agent", CoordGrid::new(3222, 0, 3218));
    let cache = rl_env::cache();

    let tutorial = cache
        .varps
        .get_by_debugname("tutorial")
        .expect("content/274 always declares %tutorial");
    let tutorial_id = tutorial.id;
    let tutorial_var_type = tutorial.var_type;

    let inv_id = cache
        .invs
        .get_by_debugname("inv")
        .expect("content/274 always declares the backpack inv")
        .id;
    let bones_id = cache
        .objs
        .get_by_debugname("bones")
        .expect("content/274 always declares bones")
        .id;

    let p = env.engine.get_player_mut(pid).expect("just spawned");

    // -- capture the DEFAULT profile: fresh spawn, tutorial at 0, empty backpack --
    let default_profile = rs_engine::player_save::extract_profile(&p.player, cache);
    assert!(
        default_profile
            .varps
            .iter()
            .find(|(id, _)| *id == tutorial_id)
            .is_none(),
        "a fresh spawn's %tutorial should be 0, hence absent from the sparse profile"
    );
    assert!(
        default_profile.invs.iter().find(|i| i.inv_type == inv_id).is_none(),
        "a fresh spawn's backpack should be empty, hence absent from the sparse profile"
    );
    let default_bytes = rs_engine::player_save::save_binary(&default_profile, cache);

    // -- dirty the live player: nonzero %tutorial + an item in the backpack --
    p.player
        .vars
        .set(tutorial_id, VarValue::from_int(tutorial_var_type, 7770));
    let inv = p
        .player
        .invs
        .entry(inv_id)
        .or_insert_with(|| Inventory::with_stack_mode(28, StackMode::Normal));
    inv.set(0, bones_id, 1);
    assert_eq!(p.player.vars.get(tutorial_id).as_int(), 7770, "setup: dirtied the varp");
    assert!(p.player.invs.get(&inv_id).unwrap().slots[0].is_some(), "setup: dirtied the inventory");

    // -- restore the DEFAULT profile: this is exactly the sequence
    //    host_load_profile runs (load_binary, then whatever clears perm
    //    state, then apply_profile) --
    let reloaded =
        rs_engine::player_save::load_binary(&default_bytes).expect("just-written default must load");
    rs_engine::player_save::clear_perm_state(&mut p.player, cache);
    rs_engine::player_save::apply_profile(&reloaded, &mut p.player, cache);

    assert_eq!(
        p.player.vars.get(tutorial_id).as_int(),
        0,
        "restoring a default profile must RESET the varp, not leave the dirtied value merged in"
    );
    assert!(
        p.player
            .invs
            .get(&inv_id)
            .map(|inv| inv.slots.iter().all(|s| s.is_none()))
            .unwrap_or(true),
        "restoring a default profile must CLEAR the inventory, not leave the item merged in"
    );
}
