//! A task: a start state, ordered milestones, a goal, and a budget.
//!
//! ★ Generalises `scenario::Scenario` from duel-shaped (two sides, Win/Loss/
//! Draw) to task-shaped (one agent, ordered progress). `Loadout` is reused
//! verbatim -- it already does stats, backpack, worn gear and varps, and it
//! already fails loud on an unknown debugname.
//!
//! ★★ RESOLUTION IS SEPARATE FROM PARSING AND BOTH FAIL LOUD. The failure
//! being designed against is a task that parses, runs, and scores zero
//! forever because a predicate names something that does not exist -- a model
//! bug that is really a typo.
//!
//! ★ Only `Deserialize` is derived below, not `Serialize` -- `scenario::
//! Loadout` (reused here verbatim) derives only `Deserialize` too, and a type
//! containing it cannot derive `Serialize` without that also being true of
//! `Loadout`. Nothing here needs to serialize a `Task` back to RON.

use crate::ontology::{EntityKind, Ontology};
use crate::scenario::{stat_index, Loadout};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Cmp { Eq, Ne, Lt, Le, Gt, Ge }

impl Cmp {
    pub fn holds(self, lhs: i64, rhs: i64) -> bool {
        match self {
            Cmp::Eq => lhs == rhs, Cmp::Ne => lhs != rhs,
            Cmp::Lt => lhs <  rhs, Cmp::Le => lhs <= rhs,
            Cmp::Gt => lhs >  rhs, Cmp::Ge => lhs >= rhs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Condition {
    Varp(String, Cmp, i32),
    VarpDelta(String, i32),
    Varbit(String, Cmp, i32),
    Stat(String, Cmp, u8),
    XpGain(String, i32),
    Inv(String, Cmp, u32),
    Worn(String),
    Coord { x: u16, z: u16, level: u8, radius: u16 },
    Timeout,
    Death,
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum At {
    Coord(u16, u8, u16),
    Npc(String),
    /// ★ Resolution only -- running this belongs with the respawn work. See
    /// the module doc on why checkpoint restore was withdrawn as a start.
    TeacherPlayed { milestone: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Start { pub at: At, pub seed: u64, pub jitter: u8, pub loadout: Loadout }

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Milestone { pub name: String, pub when: Condition }

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Task {
    pub name: String,
    pub budget_ticks: u32,
    pub start: Start,
    pub progress: Vec<Milestone>,
    pub goal: Condition,
    pub fail: Option<Condition>,
}

#[derive(Debug)]
pub enum TaskError { Io(std::io::Error), Parse(String) }

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Parse(e) => write!(f, "parse: {e}"),
        }
    }
}

/// What a resolved task needs before it can be spawned.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved { pub spot: (u16, u8, u16) }

impl Task {
    pub fn load(path: &str) -> Result<Task, TaskError> {
        let text = std::fs::read_to_string(path).map_err(TaskError::Io)?;
        ron::from_str(&text).map_err(|e| TaskError::Parse(e.to_string()))
    }

    /// Every name in the task, checked against the ontology. Returns EVERY
    /// failure, not the first -- fixing one typo at a time across a long task
    /// file is how a two-minute error becomes an afternoon.
    pub fn resolve(&self, o: &Ontology) -> Result<Resolved, Vec<String>> {
        let mut errs = Vec::new();

        let has = |kind: EntityKind, name: &str| {
            o.entities.iter().any(|e| e.kind == kind && e.name == name)
        };

        let spot = match &self.start.at {
            At::Coord(x, l, z) => Some((*x, *l, *z)),
            // ★ Resolution only. The milestone must be one this task declares,
            // so a typo here fails at load rather than producing an episode
            // that starts from nowhere.
            At::TeacherPlayed { milestone } => {
                if !self.progress.iter().any(|m| m.name == *milestone) {
                    errs.push(format!("start milestone {milestone:?} is not in this task's progress list"));
                }
                Some((0, 0, 0))
            }
            At::Npc(name) => {
                match o.entities.iter().find(|e| e.kind == EntityKind::Npc && e.name == *name) {
                    None => { errs.push(format!("start npc {name:?} is not an npc debugname")); None }
                    Some(e) => match o.spawns.get(&e.id).and_then(|v| v.first()) {
                        None => { errs.push(format!("npc {name:?} (id {}) has no map spawn", e.id)); None }
                        Some(&s) => Some(s),
                    },
                }
            }
        };

        for (i, item) in self.start.loadout.inventory.iter().enumerate() {
            if !has(EntityKind::Obj, &item.0) {
                errs.push(format!("loadout.inventory[{i}] {:?} is not an obj debugname", item.0));
            }
        }
        for (i, w) in self.start.loadout.worn.iter().enumerate() {
            if !has(EntityKind::Obj, w) {
                errs.push(format!("loadout.worn[{i}] {w:?} is not an obj debugname"));
            }
        }
        for (i, v) in self.start.loadout.vars.iter().enumerate() {
            if !has(EntityKind::Varp, &v.0) {
                errs.push(format!("loadout.vars[{i}] {:?} is not a varp debugname", v.0));
            }
        }
        for (i, s) in self.start.loadout.stats.iter().enumerate() {
            if stat_index(&s.0).is_none() {
                errs.push(format!("loadout.stats[{i}] {:?} is not a stat name", s.0));
            }
        }

        let mut check = |c: &Condition| resolve_condition(c, o, &mut errs);
        for m in &self.progress { check(&m.when); }
        check(&self.goal);
        if let Some(f) = &self.fail { check(f); }

        match (errs.is_empty(), spot) {
            (true, Some(spot)) => Ok(Resolved { spot }),
            _ => Err(errs),
        }
    }
}

fn resolve_condition(c: &Condition, o: &Ontology, errs: &mut Vec<String>) {
    let has = |kind: EntityKind, name: &str| {
        o.entities.iter().any(|e| e.kind == kind && e.name == name)
    };
    match c {
        Condition::Varp(n, _, _) | Condition::VarpDelta(n, _) => {
            // ★ A varbit named here is a real authoring error worth its own
            // message: `%` hides the difference in source, so "it works in the
            // scripts" is not evidence it is a varp.
            if !has(EntityKind::Varp, n) {
                if has(EntityKind::Varbit, n) {
                    errs.push(format!("{n:?} is a varbit, not a varp -- use Varbit(..)"));
                } else {
                    errs.push(format!("varp {n:?} does not exist"));
                }
            }
        }
        Condition::Varbit(n, _, _) => {
            if !has(EntityKind::Varbit, n) { errs.push(format!("varbit {n:?} does not exist")); }
        }
        Condition::Inv(n, _, _) | Condition::Worn(n) => {
            if !has(EntityKind::Obj, n) { errs.push(format!("obj {n:?} does not exist")); }
        }
        Condition::Stat(n, _, _) | Condition::XpGain(n, _) => {
            if stat_index(n).is_none() { errs.push(format!("stat {n:?} does not exist")); }
        }
        Condition::Coord { .. } | Condition::Timeout | Condition::Death => {}
        Condition::All(v) | Condition::Any(v) => {
            for c in v { resolve_condition(c, o, errs); }
        }
        Condition::Not(c) => resolve_condition(c, o, errs),
    }
}
