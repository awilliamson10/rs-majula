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
pub mod xref;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntityKind {
    Varp,
    Varbit,
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
