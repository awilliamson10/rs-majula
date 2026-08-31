//! Generates the "two item" profile fixture that regresses the third round of
//! the client-inventory-desync bug class (final-review Critical finding):
//! `host/src/profile-once.ts` loads THIS fixture first (two occupied
//! backpack slots), then the existing one-slot `changed-profile.bin`
//! (`gen_changed_profile.rs`) over it, and the probe asserts slot 1 goes back
//! to empty on the CLIENT, not just in the engine.
//!
//! ★★ ONE ENGINE PER PROCESS, THEREFORE ONE `#[test]` FN IN THIS BINARY. See
//! `gen_changed_profile.rs`'s own header for why `EnvHarness::boot_seeded`
//! trips this even without going through `host_new`.
//!
//! # ★★ WHY A SEPARATE FIXTURE, NOT A THIRD SLOT ON THE EXISTING ONE
//! The bug this regresses is specifically about a restore whose profile
//! carries FEWER items than the live session holds -- `apply_profile`
//! replaces the `Inventory` object wholesale and only marks the slots IT
//! writes as dirty (see `clear_perm_state`'s doc comment in
//! `player_save.rs`), so a slot the profile doesn't mention is never told to
//! the client. Adding a slot to `changed-profile.bin` would not exercise
//! that: the probe needs to go from MORE occupied slots to FEWER across a
//! single restore, which means two distinct fixtures loaded in sequence.
use rs_grid::CoordGrid;
use rs_inv::{Inventory, StackMode};

/// Lumbridge — the same mainland spawn `profile-once.ts` boots at and the
/// same coordinate `gen_changed_profile.rs`'s fixture uses, so loading either
/// fixture never also moves the player and confounds the position channel.
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/test/fixtures/two-item-profile.bin")
}

#[test]
fn generates_a_two_item_profile_fixture() {
    let (x, level, z) = LUMBRIDGE;
    let mut env = rl_env::EnvHarness::boot_seeded(4242);
    let (pid, _rx) = env.engine.spawn_player_tapped("agent", CoordGrid::new(x, level, z));
    let cache = rl_env::cache();

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

    let inv = p
        .player
        .invs
        .entry(inv_id)
        .or_insert_with(|| Inventory::with_stack_mode(28, StackMode::Normal));
    // ★ TWO slots, not one: slot 0 is what the one-slot fixture ALSO carries
    // (so it survives the restore this test's TS half performs), slot 1 is
    // what that restore must clear.
    inv.set(0, bones_id, 1);
    inv.set(1, bones_id, 1);

    let profile = rs_engine::player_save::extract_profile(&p.player, cache);
    let inv_profile = profile
        .invs
        .iter()
        .find(|i| i.inv_type == inv_id)
        .expect("extract_profile did not capture the inv we just set");
    assert_eq!(inv_profile.items, vec![(0u16, bones_id, 1u32), (1u16, bones_id, 1u32)]);

    let bytes = rs_engine::player_save::save_binary(&profile, cache);
    assert!(!bytes.is_empty());

    let reloaded = rs_engine::player_save::load_binary(&bytes).expect("just-written fixture must load");
    assert_eq!(reloaded.x, x);
    assert_eq!(reloaded.z, z);
    assert_eq!(
        reloaded.invs.iter().find(|i| i.inv_type == inv_id).map(|i| i.items.clone()),
        Some(vec![(0u16, bones_id, 1u32), (1u16, bones_id, 1u32)])
    );

    let path = fixture_path();
    std::fs::create_dir_all(path.parent().unwrap()).expect("create host/test/fixtures");
    std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}
