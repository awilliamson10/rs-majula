//! Task 4: npc spawn coordinates, so a task file can say `npc: "cook"` instead
//! of a magic number a human had to go and find.
//!
//! Format, read from `content/274/maps/m29_75.jm2`: sections are
//! `==== MAP ====`, `==== LOC ====`, `==== NPC ====`, `==== OBJ ====`, and NPC
//! lines are `level x z: npcid` with x/z LOCAL to the map square. The filename
//! `m<mx>_<mz>.jm2` carries the square, so absolute = (mx*64 + x, mz*64 + z).

use rl_env::ontology::spawns;

fn scan() -> std::collections::BTreeMap<u16, Vec<(u16, u8, u16)>> {
    let maps = rl_env::content_root().join(rs_pack::CONTENT_DIR).join("maps");
    spawns::scan(&maps)
}

/// ★★ THE MULTIPLY IS THE BUG THIS GUARDS. Forgetting `mx * 64` yields
/// coordinates that are plausible integers in 0..63 and land the agent in the
/// sea. Asserting the maximum is far above 63 catches it without needing to
/// know any specific answer.
#[test]
fn coordinates_are_absolute_not_local() {
    let all = scan();
    assert!(!all.is_empty(), "no npc spawns parsed at all");

    let max_x = all.values().flatten().map(|s| s.0).max().unwrap();
    let max_z = all.values().flatten().map(|s| s.2).max().unwrap();
    assert!(max_x > 1000, "max x is {max_x} -- the mx*64 multiply is missing");
    assert!(max_z > 1000, "max z is {max_z} -- the mz*64 multiply is missing");

    for (id, spots) in &all {
        for &(_, level, _) in spots {
            assert!(level < 4, "npc {id} spawned on level {level}");
        }
    }
}

#[test]
fn the_cook_has_a_spawn() {
    let cache = rl_env::cache();
    let cook = cache.npcs.get_by_debugname("cook").expect("npc `cook`");
    let all = scan();
    let spots = all.get(&cook.id).unwrap_or_else(|| {
        panic!("npc `cook` (id {}) has no spawn in any map square", cook.id)
    });
    assert!(!spots.is_empty());
}

#[test]
fn scanning_is_deterministic() {
    assert_eq!(scan(), scan());
}
