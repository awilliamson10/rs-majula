//! The rev-274 ontology: every entity the engine runs, joined against what the
//! content source says it means.
//!
//! ★★ THE ENTITY REGISTRY AND THE MEANING REGISTRY ARE DIFFERENT FILES JOINED
//! BY NOTHING, and that is the defect this module exists to close.
//! `quest_cook.varp` is two lines (`[cookquest]` / `scope=perm`); the value `1`
//! is a bare literal in `quest_cook.rs2:32` named in no `.constant` file at all,
//! and `^cook_complete = 2` lives in a different subtree
//! (`general/configs/quest.constant:12`). Reading a varp's own config directory
//! tells you nothing about its states.
//!
//! ★ What this does NOT need to check: config stanzas against the pack
//! registry. `rs-pack/src/pack/config/varp.rs` walks `0..pack.max` and panics
//! via `pack.get_by_id` on an id gap and via `configs.get(debugname)` on a name
//! with no stanza, then copies the name into the cache. That invariant is
//! already enforced at pack time, on every boot; re-asserting it here would be
//! tautological.

pub mod dump;
pub mod spawns;
pub mod xref;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntityKind {
    Varp,
    Varbit,
    /// ★★ `varn` and `vars` exist and RuneScript spells them `%name` too.
    /// `rs-runec`'s single `GameVar` symbol kind covers varp | varn | vars |
    /// varbit, so a source scan cannot tell the four apart and an ontology
    /// carrying only varps reports the other three as unresolved.
    Varn,
    Vars,
    Obj,
    Npc,
    Loc,
    Inv,
    Enum,
    Struct,
    Param,
    Seq,
}

/// One cache entity that carries a debugname.
///
/// `fields` is stringly typed on purpose: the artifact exists to be read and to
/// resolve names, and a string bag stays additive when a new field turns out to
/// matter. `BTreeMap` (not `HashMap`) so the committed artifact diffs cleanly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Entity {
    pub kind: EntityKind,
    pub id: u16,
    pub name: String,
    pub fields: BTreeMap<String, String>,
}

use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ontology {
    pub entities: Vec<Entity>,
    pub xref: xref::Xref,
    pub spawns: BTreeMap<u16, Vec<(u16, u8, u16)>>,
}

pub fn build() -> Ontology {
    // ★ `crate::`, not `rl_env::` -- inside the crate itself, pathing through
    // its own name does not resolve without `extern crate self as rl_env`.
    let content = crate::content_root().join(rs_pack::CONTENT_DIR);
    Ontology {
        entities: dump::dump_cache(),
        xref: xref::scan(&content),
        spawns: spawns::scan(&content.join("maps")),
    }
}

/// What the packer does NOT already guarantee.
///
/// ★★ This CATEGORISES rather than passes or fails. A `%name` that resolves to
/// a varbit is correct, not a mismatch -- RuneScript spells both identically —
/// and a varp no script mentions is a reachability observation, not an error.
/// Only `unresolved_refs` and `unresolved_constants` are defects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// `%name` in source that is neither a varp nor a varbit.
    pub unresolved_refs: Vec<String>,
    /// Cache varps no script mentions -- candidates for "dead in rev 274".
    pub unreferenced: Vec<String>,
    /// `%name` that resolved to a varbit, varn or vars rather than a varp.
    /// Expected, and the reason `unresolved_refs` is not simply "everything
    /// that missed" -- all four share the `%` sigil.
    pub varbit_refs: Vec<String>,
    /// `%v = ^c` where `^c` is defined nowhere -- each silently drops a state
    /// from `values_assigned_to`.
    pub unresolved_constants: Vec<String>,
}

pub fn report(o: &Ontology) -> Report {
    let cache = crate::cache();
    let mut r = Report::default();

    let mut referenced: BTreeSet<&str> = BTreeSet::new();
    for x in &o.xref.varp_refs {
        referenced.insert(x.varp.as_str());
    }
    for name in &referenced {
        if cache.varps.get_by_debugname(name).is_some() {
            continue;
        }
        if cache.varbits.get_by_debugname(name).is_some()
            || cache.varns.get_by_debugname(name).is_some()
            || cache.varss.get_by_debugname(name).is_some()
        {
            r.varbit_refs.push((*name).to_string());
        } else {
            r.unresolved_refs.push((*name).to_string());
        }
    }

    for e in o.entities.iter().filter(|e| e.kind == EntityKind::Varp) {
        if !referenced.contains(e.name.as_str()) {
            r.unreferenced.push(format!("{} (id {})", e.name, e.id));
        }
    }

    for x in &o.xref.varp_refs {
        let Some(w) = &x.write else { continue };
        let Some(c) = &w.constant else { continue };
        if !o.xref.constants.contains_key(c) && !o.xref.non_integer_constants.contains(c) {
            r.unresolved_constants.push(format!("%{} = ^{c}", x.varp));
        }
    }

    r.unresolved_refs.sort();
    r.unresolved_refs.dedup();
    r.unreferenced.sort();
    r.unreferenced.dedup();
    r.varbit_refs.sort();
    r.varbit_refs.dedup();
    r.unresolved_constants.sort();
    r.unresolved_constants.dedup();
    r
}
