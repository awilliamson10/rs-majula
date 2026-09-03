//! Walks every debugnamed entity in the packed cache.
//!
//! ★ Uses `crate::cache()`, NOT `shared_cache()`: the dump wants the
//! `CacheStore` only, and `shared_cache` additionally rebuilds a fresh
//! `ScriptProvider` on every call (see its doc comment in `lib.rs`).
//!
//! ★ No engine is booted here and none should be -- `CLAUDE.md` #2. The cache
//! is memoized and engine-free, which is what lets these tests run in the same
//! process as every other `rl-env` test.
//!
//! ★ `TypeProvider`'s `debugnames` field is public (`rs-pack/src/cache/
//! provider.rs`), so this iterates it directly. An earlier draft of the plan
//! added an accessor first, on the false claim that it was private.

use super::{Entity, EntityKind};
use std::collections::BTreeMap;

fn put(map: &mut BTreeMap<String, String>, k: &str, v: impl ToString) {
    map.insert(k.to_string(), v.to_string());
}

/// The menu options an npc or loc offers, comma-joined. These are what a task
/// author names an interaction by, and what the TypeScript side's `matchesOp`
/// matches against.
fn ops_field(op: &Option<Box<[Option<Box<str>>]>>) -> Option<String> {
    let ops = op.as_ref()?;
    let joined: Vec<&str> = ops.iter().filter_map(|o| o.as_deref()).collect();
    if joined.is_empty() {
        None
    } else {
        Some(joined.join(","))
    }
}

pub fn dump_cache() -> Vec<Entity> {
    let c = crate::cache();
    let mut out: Vec<Entity> = Vec::new();

    for (name, &id) in c.varps.debugnames.iter() {
        let Some(t) = c.varps.get_by_id(id) else { continue };
        let mut f = BTreeMap::new();
        put(&mut f, "scope", format!("{:?}", t.scope).to_lowercase());
        put(&mut f, "var_type", format!("{:?}", t.var_type).to_lowercase());
        put(&mut f, "transmit", t.transmit);
        put(&mut f, "protect", t.protect);
        out.push(Entity { kind: EntityKind::Varp, id, name: name.to_string(), fields: f });
    }

    // ★★ VARBITS ARE NOT OPTIONAL. RuneScript spells a varp and a varbit
    // identically (`%name`), so an ontology without varbits reports phantom
    // unresolved references and cannot express a quest whose whole progress
    // record is bit-packed -- `horror_journal.rs2` branches on seven varbits
    // and zero varps.
    for (name, &id) in c.varbits.debugnames.iter() {
        let Some(t) = c.varbits.get_by_id(id) else { continue };
        let mut f = BTreeMap::new();
        put(&mut f, "basevar", t.basevar);
        put(&mut f, "start_bit", t.start_bit);
        put(&mut f, "end_bit", t.end_bit);
        out.push(Entity { kind: EntityKind::Varbit, id, name: name.to_string(), fields: f });
    }

    // ★ Same sigil, different kind. Without these the join reports every
    // shop, quest-progress and scratch variable as unresolved.
    for (name, &id) in c.varns.debugnames.iter() {
        out.push(Entity { kind: EntityKind::Varn, id, name: name.to_string(), fields: BTreeMap::new() });
    }
    for (name, &id) in c.varss.debugnames.iter() {
        out.push(Entity { kind: EntityKind::Vars, id, name: name.to_string(), fields: BTreeMap::new() });
    }

    for (name, &id) in c.objs.debugnames.iter() {
        let Some(t) = c.objs.get_by_id(id) else { continue };
        let mut f = BTreeMap::new();
        put(&mut f, "stackable", t.stackable);
        if let Some(n) = t.name.as_deref() {
            put(&mut f, "name", n);
        }
        // `WearPos` is an enum; Debug is the stable rendering here.
        if let Some(w) = t.wearpos {
            put(&mut f, "wearpos", format!("{w:?}"));
        }
        out.push(Entity { kind: EntityKind::Obj, id, name: name.to_string(), fields: f });
    }

    for (name, &id) in c.npcs.debugnames.iter() {
        let Some(t) = c.npcs.get_by_id(id) else { continue };
        let mut f = BTreeMap::new();
        put(&mut f, "size", t.size);
        if let Some(n) = t.name.as_deref() {
            put(&mut f, "name", n);
        }
        if let Some(ops) = ops_field(&t.op) {
            put(&mut f, "ops", ops);
        }
        out.push(Entity { kind: EntityKind::Npc, id, name: name.to_string(), fields: f });
    }

    for (name, &id) in c.locs.debugnames.iter() {
        let Some(t) = c.locs.get_by_id(id) else { continue };
        let mut f = BTreeMap::new();
        put(&mut f, "width", t.width);
        put(&mut f, "length", t.length);
        if let Some(n) = t.name.as_deref() {
            put(&mut f, "name", n);
        }
        if let Some(ops) = ops_field(&t.op) {
            put(&mut f, "ops", ops);
        }
        out.push(Entity { kind: EntityKind::Loc, id, name: name.to_string(), fields: f });
    }

    for (kind, names) in [
        (EntityKind::Inv, c.invs.debugnames.iter().collect::<Vec<_>>()),
        (EntityKind::Enum, c.enums.debugnames.iter().collect::<Vec<_>>()),
        (EntityKind::Struct, c.structs.debugnames.iter().collect::<Vec<_>>()),
        (EntityKind::Param, c.params.debugnames.iter().collect::<Vec<_>>()),
        (EntityKind::Seq, c.seqs.debugnames.iter().collect::<Vec<_>>()),
    ] {
        for (name, &id) in names {
            out.push(Entity { kind, id, name: name.to_string(), fields: BTreeMap::new() });
        }
    }

    // ★ Sorted so the committed artifact diffs cleanly. `Entity`'s derived Ord
    // is (kind, id, name, fields), which is stable across runs because ids are.
    out.sort();
    out
}
