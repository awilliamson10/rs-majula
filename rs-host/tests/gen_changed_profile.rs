//! Generates the "changed" profile fixture the client-resync probe
//! (`host/src/profile-once.ts`) loads to measure the channels a naive restore
//! can silently fail to reach: `%tutorial`, the inventory, and (Task 2c's
//! varp-resync fix) a TRANSMITTABLE perm varp, `option_nodef`.
//!
//! ★★ ONE ENGINE PER PROCESS, THEREFORE ONE `#[test]` FN IN THIS BINARY.
//! `host_new`/`EnvHarness::boot_seeded` aborts on a second call in a process
//! (`rs-pathfinder`'s `COLLISION_FLAGS` is process-global) and `bun test`
//! equivalent discipline applies here too — see `profile.rs`'s own header.
//!
//! # ★ WHY A RUST TEST WRITES A FILE, RATHER THAN A HAND-ROLLED TS BLOB
//! `player_save::save_binary`'s format has a CRC32 trailer and a varp array
//! sized to the LIVE CACHE's `varps.count()` — reimplementing either in
//! TypeScript would mean re-deriving the encoder from scratch with nothing
//! checking the result against the real one. This test calls the exact same
//! `extract_profile`/`save_binary` pair `host_save_profile` calls, so the
//! fixture is guaranteed well-formed, and it round-trips the result through
//! `load_binary` before writing it, so a format change fails HERE rather than
//! as a mystery `loadProfile` rejection three processes away.
//!
//! # ★ WHY THE CHANGE IS MADE BY POKING `player.vars`/`player.invs` DIRECTLY,
//! NOT VIA A `ClientCheat` (`::setvar`, `::give`)
//! Those commands are gated to `StaffModLevel::Developer`, whose default is
//! `#[cfg(debug_assertions)]` (`rs-entity/src/player.rs:295-297`) — FALSE in
//! the `--release` build the TS probe's `HOST_DYLIB` points at, so a spawned
//! player there is `Normal` and every developer cheat silently no-ops
//! (`cheat_developer` falls through to lower tiers rather than erroring — see
//! `rs-engine/src/handlers/client_cheat.rs`). Poking the fields directly is
//! also exactly how `rs-host/tests/mainland_login_grants_tabs.rs` builds ITS
//! assertions, so it is a precedented technique in this suite, not a new one.
//!
//! # ★★ THIS FIXTURE DOES NOT PROVE THE CLIENT RECEIVES ANYTHING
//! It only proves the bytes are well-formed and describe the values this file
//! documents below. Whether `host_load_profile` gets them to the CLIENT is
//! exactly the question `profile-once.ts` answers, by loading this file into
//! a live, booted client and reading `state()`/`client.*` afterwards.

use rs_grid::CoordGrid;
use rs_inv::{Inventory, StackMode};
use rs_pack::cache::VarValue;

/// Lumbridge — the same mainland spawn `profile-once.ts` boots at, so loading
/// this fixture during the probe does not ALSO move the player and confound
/// the position channel (already covered separately by the existing
/// teleport-and-restore check, which Task 2b's `host_load_profile` fix is
/// about).
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

/// A tutorial value distinct from every value a fresh mainland spawn (0) or
/// Tutorial Island's very first steps (1, 130, 195, ...) would ever produce —
/// see `content/274/scripts/tutorial/configs/tutorial.constant` — so a
/// comparison against it is never accidentally vacuous.
const CHANGED_TUTORIAL: i32 = 7770;

/// `option_nodef`'s cache id in `content/274`'s packed `varp.dat` — measured,
/// not assumed, via `cache.varps.get_by_debugname("option_nodef").id` when
/// this fixture was authored. ★ PINNED HERE SO A DRIFT FAILS LOUDLY: the
/// TS-side probe (`host/src/profile-once.ts`) has no debugname table to
/// resolve this id from at runtime (`VarpType.ts`'s client-side decoder
/// discards the debugname string, `code === 10 => dat.gjstr()` with the
/// result unused) and reads `client.var[172]` as a bare literal instead. The
/// assertion below is what makes a future cache repack that reorders
/// `player_controls.varp` fail HERE, in the crate that can still resolve the
/// name, rather than as a silently-wrong index three processes away.
const OPTION_NODEF_ID: u16 = 172;

/// `option_nodef` (`content/274/scripts/interface_controls/configs/
/// player_controls.varp`: `scope=perm`, `transmit=yes`) is one of the
/// `scope=perm` + `transmit=yes` varps a reviewer confirmed real content
/// exercises. A fresh player never sets it, so it starts at 0 — this is a
/// value a restore's varp resync must carry to the client, distinct from
/// `%tutorial` (which has no `transmit` at all, hence no client-side channel)
/// and the inventory (a different resync mechanism entirely, fixed
/// separately — see `Inventory::clear()`'s doc comment in `player_save.rs`).
const CHANGED_OPTION_NODEF: i32 = 1;

/// Where the probe expects to find the generated fixture: `host/test/
/// fixtures/changed-profile.bin`, i.e. two directories up from this crate
/// (`majula/rs-host` -> `majula` -> `rs-vla`) and into `host/test/fixtures`.
/// Computed from `CARGO_MANIFEST_DIR` rather than a bare relative path so
/// `cargo test` works from any invocation directory.
fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/test/fixtures/changed-profile.bin")
}

#[test]
fn generates_a_changed_profile_fixture_with_a_nonzero_tutorial_an_item_and_a_transmit_varp() {
    let (x, level, z) = LUMBRIDGE;
    let mut env = rl_env::EnvHarness::boot_seeded(4242);
    let (pid, _rx) = env.engine.spawn_player_tapped("agent", CoordGrid::new(x, level, z));
    // `boot_seeded` already populated the process's memoized cache cell; this
    // just borrows that same instance — see `host_new_at`'s doc comment.
    let cache = rl_env::cache();

    let tutorial = cache
        .varps
        .get_by_debugname("tutorial")
        .expect("content/274 always declares %tutorial");
    let tutorial_id = tutorial.id;
    let tutorial_var_type = tutorial.var_type;

    let nodef = cache
        .varps
        .get_by_debugname("option_nodef")
        .expect("content/274 always declares option_nodef");
    assert_eq!(
        nodef.id, OPTION_NODEF_ID,
        "option_nodef's cache id moved -- update OPTION_NODEF_ID here AND the \
         matching literal in host/src/profile-once.ts, or the TS probe silently \
         reads the wrong varp"
    );

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

    p.player
        .vars
        .set(tutorial_id, VarValue::from_int(tutorial_var_type, CHANGED_TUTORIAL));
    p.player
        .vars
        .set(nodef.id, VarValue::from_int(nodef.var_type, CHANGED_OPTION_NODEF));

    let inv = p
        .player
        .invs
        .entry(inv_id)
        .or_insert_with(|| Inventory::with_stack_mode(28, StackMode::Normal));
    inv.set(0, bones_id, 1);

    let profile = rs_engine::player_save::extract_profile(&p.player, cache);
    assert_eq!(
        profile
            .varps
            .iter()
            .find(|(id, _)| *id == tutorial_id)
            .map(|(_, v)| *v),
        Some(CHANGED_TUTORIAL),
        "extract_profile did not capture the tutorial varp we just set"
    );
    assert_eq!(
        profile
            .varps
            .iter()
            .find(|(id, _)| *id == nodef.id)
            .map(|(_, v)| *v),
        Some(CHANGED_OPTION_NODEF),
        "extract_profile did not capture the option_nodef varp we just set"
    );
    let inv_profile = profile
        .invs
        .iter()
        .find(|i| i.inv_type == inv_id)
        .expect("extract_profile did not capture the inv we just set");
    assert_eq!(inv_profile.items, vec![(0u16, bones_id, 1u32)]);

    let bytes = rs_engine::player_save::save_binary(&profile, cache);
    assert!(!bytes.is_empty());

    // Round-trip through the exact reader `host_load_profile` uses.
    let reloaded = rs_engine::player_save::load_binary(&bytes).expect("just-written fixture must load");
    assert_eq!(reloaded.x, x);
    assert_eq!(reloaded.z, z);
    assert_eq!(
        reloaded.varps.iter().find(|(id, _)| *id == tutorial_id).map(|(_, v)| *v),
        Some(CHANGED_TUTORIAL)
    );
    assert_eq!(
        reloaded.varps.iter().find(|(id, _)| *id == nodef.id).map(|(_, v)| *v),
        Some(CHANGED_OPTION_NODEF)
    );
    assert_eq!(
        reloaded.invs.iter().find(|i| i.inv_type == inv_id).map(|i| i.items.clone()),
        Some(vec![(0u16, bones_id, 1u32)])
    );

    let path = fixture_path();
    std::fs::create_dir_all(path.parent().unwrap()).expect("create host/test/fixtures");
    std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}
