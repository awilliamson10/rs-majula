//! Task 2: the cache dump is the "what the engine actually runs" half of the
//! ontology.
//!
//! ★ Assertions are on NAMED entities this project depends on, never on total
//! counts. A provider's `count()` includes unnamed slots and is not the same
//! number as the content tree's config stanzas -- `obj.pack` has 3,894 entries
//! against 2,629 non-`cert_` stanzas, because `rs-pack/src/pack/config/obj.rs`
//! synthesises 1,265 `cert_*` objs at pack time from the debugname alone.

use rl_env::ontology::{dump, EntityKind};

#[test]
fn dumps_the_named_entities_tasks_depend_on() {
    let entities = dump::dump_cache();

    let find = |kind: EntityKind, name: &str| {
        entities
            .iter()
            .find(|e| e.kind == kind && e.name == name)
            .unwrap_or_else(|| panic!("{kind:?} {name:?} missing from dump"))
    };

    let tutorial = find(EntityKind::Varp, "tutorial");
    assert_eq!(tutorial.fields.get("scope").map(String::as_str), Some("perm"));

    find(EntityKind::Varp, "cookquest");
    find(EntityKind::Npc, "cook");
    for obj in ["pot_flour", "egg", "bucket_milk"] {
        find(EntityKind::Obj, obj);
    }
}

/// ★★ `horrorquest` is a VARBIT, not a varp -- bits of `deephorror`. RuneScript
/// spells both `%name`, so without varbits in the dump the xref pass reports
/// phantom mismatches and four quests are unauthorable.
#[test]
fn varbits_are_dumped_alongside_their_base_varp() {
    let entities = dump::dump_cache();

    let varbit = entities
        .iter()
        .find(|e| e.kind == EntityKind::Varbit && e.name == "horrorquest")
        .expect("varbit horrorquest missing from dump");
    let base = varbit.fields.get("basevar").expect("varbit carries no basevar");

    // The base varp must itself be in the dump, by id.
    let base_id: u16 = base.parse().expect("basevar is not a number");
    assert!(
        entities
            .iter()
            .any(|e| e.kind == EntityKind::Varp && e.id == base_id),
        "varbit horrorquest's basevar {base_id} is not a dumped varp",
    );
    assert!(varbit.fields.contains_key("start_bit"));
    assert!(varbit.fields.contains_key("end_bit"));
}

#[test]
fn obj_fields_carry_what_a_loadout_needs() {
    let entities = dump::dump_cache();
    // `apply_loadout_stats_inv` resolves worn gear through the cache's own
    // `wearpos` and panics without it (rl-env/src/lib.rs). If the dump does not
    // carry it, a task file cannot be validated before it runs.
    let helm = entities
        .iter()
        .find(|e| e.kind == EntityKind::Obj && e.name == "rune_full_helm")
        .expect("rune_full_helm in dump");
    assert!(
        helm.fields.contains_key("wearpos"),
        "wearpos missing: {:?}",
        helm.fields,
    );
}

/// ★ The npc's menu ops are what a task author needs to name an interaction
/// ("Talk-to", "Attack"), and what `matchesOp` matches against on the TS side.
#[test]
fn npc_ops_are_dumped() {
    let entities = dump::dump_cache();
    let cook = entities
        .iter()
        .find(|e| e.kind == EntityKind::Npc && e.name == "cook")
        .expect("npc cook");
    let ops = cook.fields.get("ops").expect("cook carries no ops");
    assert!(ops.contains("Talk-to"), "cook's ops were {ops:?}");
}

#[test]
fn the_dump_is_deterministic() {
    // The artifact is committed and diffed; a nondeterministic dump makes every
    // regeneration a spurious diff and hides the real ones.
    assert_eq!(dump::dump_cache(), dump::dump_cache());
}
