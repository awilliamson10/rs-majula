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
    /// Model calls before the episode ends.
    ///
    /// ★ THE AGENT'S CLOCK, where `budget_ticks` is the engine's. A turn is a
    /// millisecond input schedule spanning an arbitrary number of server ticks,
    /// and the cost driver is images per request, not ticks (see the baseline
    /// harness design §3.1). `budget_ticks` stays as the backstop, because one
    /// turn may legally contain `wait(600000)`.
    pub budget_turns: u32,
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

use std::collections::HashMap;

/// ★ THE 64-MILESTONE CEILING, STATED RATHER THAN DISCOVERED. A `u64` mask is
/// what keeps [`Armed::fold`] allocation-free on the tick path. Tutorial Island
/// has thirteen. A task wanting more needs a different carrier, and it is
/// refused at arm time rather than silently truncated.
pub const MAX_MILESTONES: usize = 64;

/// What the world looked like when the task was armed. Only the DELTA
/// conditions need it -- `VarpDelta` and `XpGain` are the two that are
/// meaningless as absolutes.
#[derive(Debug)]
struct Baseline {
    varps: HashMap<String, i32>,
    xp: Vec<i32>,
    start_tick: u64,
}

/// A resolved task, bound to one player, folding every tick.
///
/// ★ `Debug` is derived so `Task::arm`'s `Result<Armed, String>` satisfies
/// `expect_err` in the >64-milestone test -- `Result::expect_err` requires
/// the `Ok` side to be `Debug` even though that branch is never printed.
#[derive(Debug)]
pub struct Armed {
    task: Task,
    base: Baseline,
    latched: u64,
    raw: u64,
    goal: bool,
    failed: bool,
    turns: u32,
}

impl Task {
    /// Binds this task to `pid` and snapshots the baselines the delta
    /// conditions need. Fails when the task declares more milestones than the
    /// mask can carry.
    pub fn arm(self, env: &crate::EnvHarness, pid: u16) -> Result<Armed, String> {
        if self.progress.len() > MAX_MILESTONES {
            return Err(format!(
                "task {:?} declares {} milestones; the mask carries at most {}",
                self.name,
                self.progress.len(),
                MAX_MILESTONES
            ));
        }
        let cache = crate::cache();
        let mut varps = HashMap::new();
        let mut xp = Vec::new();
        if let Some(active) = env.engine.get_player(pid) {
            xp = active.player.stats.xp.to_vec();
            // Snapshot every varp any VarpDelta in this task names.
            let mut names = Vec::new();
            collect_delta_varps(&self.goal, &mut names);
            if let Some(f) = &self.fail {
                collect_delta_varps(f, &mut names);
            }
            for m in &self.progress {
                collect_delta_varps(&m.when, &mut names);
            }
            for n in names {
                if let Some(v) = cache.varps.get_by_debugname(&n) {
                    varps.insert(n, active.player.vars.get(v.id).as_int());
                }
            }
        }
        Ok(Armed {
            task: self,
            base: Baseline { varps, xp, start_tick: env.clock() },
            latched: 0,
            raw: 0,
            goal: false,
            failed: false,
            turns: 0,
        })
    }
}

fn collect_delta_varps(c: &Condition, out: &mut Vec<String>) {
    match c {
        Condition::VarpDelta(name, _) => out.push(name.clone()),
        Condition::All(cs) | Condition::Any(cs) => {
            for c in cs {
                collect_delta_varps(c, out);
            }
        }
        Condition::Not(c) => collect_delta_varps(c, out),
        _ => {}
    }
}

impl Armed {
    /// ★★ CALLED AT THE END OF EVERY `host_step`, NOT AT TURN BOUNDARIES. A
    /// turn is an input schedule spanning many ticks, and a condition can go
    /// true and false inside one. Sampling at the turn boundary misses it and
    /// presents as "the model never did that step" -- a metric bug wearing a
    /// model bug's clothes.
    pub fn fold(&mut self, env: &crate::EnvHarness, pid: u16) {
        let mut raw = 0u64;
        for (i, m) in self.task.progress.iter().enumerate() {
            if self.eval(&m.when, env, pid) {
                raw |= 1u64 << i;
            }
        }
        self.raw = raw;
        self.latched |= raw;
        if !self.goal && self.eval(&self.task.goal, env, pid) {
            self.goal = true;
        }
        if !self.failed {
            if let Some(f) = &self.task.fail {
                if self.eval(f, env, pid) {
                    self.failed = true;
                }
            }
        }
    }

    pub fn latched(&self) -> u64 { self.latched }
    pub fn raw(&self) -> u64 { self.raw }
    pub fn goal(&self) -> bool { self.goal }
    pub fn failed(&self) -> bool { self.failed }
    pub fn turns(&self) -> u32 { self.turns }
    pub fn note_turn(&mut self) { self.turns = self.turns.saturating_add(1); }
    pub fn turns_exhausted(&self) -> bool { self.turns >= self.task.budget_turns }
    pub fn milestone_names(&self) -> Vec<&str> {
        self.task.progress.iter().map(|m| m.name.as_str()).collect()
    }

    /// ★★ NEVER PANICS. This runs behind `host_step`, and every panic in an
    /// `extern "C"` frame aborts the process with no JS-visible error. An
    /// unresolvable name is `false` -- `Task::resolve` already failed loud at
    /// load, so reaching here means the cache changed under a resolved task.
    fn eval(&self, c: &Condition, env: &crate::EnvHarness, pid: u16) -> bool {
        let Some(active) = env.engine.get_player(pid) else {
            // A departed player satisfies Death and nothing else.
            return matches!(c, Condition::Death);
        };
        let cache = crate::cache();
        match c {
            Condition::Varp(name, cmp, want) => match cache.varps.get_by_debugname(name) {
                Some(v) => cmp.holds(active.player.vars.get(v.id).as_int() as i64, *want as i64),
                None => false,
            },
            Condition::VarpDelta(name, want) => match cache.varps.get_by_debugname(name) {
                Some(v) => {
                    let now = active.player.vars.get(v.id).as_int();
                    let was = self.base.varps.get(name).copied().unwrap_or(0);
                    (now as i64 - was as i64) >= *want as i64
                }
                None => false,
            },
            Condition::Varbit(name, cmp, want) => match cache.varbits.get_by_debugname(name) {
                Some(vb) => {
                    let raw = active.player.vars.get(vb.basevar).as_int() as u32;
                    // end_bit is INCLUSIVE, so a one-bit varbit has
                    // start_bit == end_bit. Guard the 32-bit case, where the
                    // shift would overflow.
                    let width = vb.end_bit.saturating_sub(vb.start_bit) as u32;
                    let mask = if width >= 31 { u32::MAX } else { (1u32 << (width + 1)) - 1 };
                    let val = (raw >> vb.start_bit) & mask;
                    cmp.holds(val as i64, *want as i64)
                }
                None => false,
            },
            Condition::Stat(name, cmp, want) => match crate::scenario::stat_index(name) {
                Some(i) => cmp.holds(active.player.stats.levels[i] as i64, *want as i64),
                None => false,
            },
            Condition::XpGain(name, want) => match crate::scenario::stat_index(name) {
                Some(i) => {
                    let now = active.player.stats.xp[i] as i64;
                    let was = self.base.xp.get(i).copied().unwrap_or(0) as i64;
                    (now - was) >= *want as i64
                }
                None => false,
            },
            Condition::Inv(name, cmp, want) => {
                let Some(obj) = cache.objs.get_by_debugname(name) else { return false };
                let Some(inv) = cache
                    .invs
                    .get_by_debugname("inv")
                    .and_then(|i| active.player.invs.get(&i.id))
                else {
                    return false;
                };
                cmp.holds(inv.total(obj.id) as i64, *want as i64)
            }
            Condition::Worn(name) => {
                let Some(obj) = cache.objs.get_by_debugname(name) else { return false };
                // ★ 94 mirrors `apply_loadout_stats_inv`'s own fallback for the
                // worn container; keeping the two identical is what stops a
                // loadout and a condition disagreeing about where gear lives.
                let worn_id = cache.invs.get_by_debugname("worn").map(|i| i.id).unwrap_or(94);
                active
                    .player
                    .invs
                    .get(&worn_id)
                    .is_some_and(|w| w.total(obj.id) > 0)
            }
            Condition::Coord { x, z, level, radius } => {
                let c0 = active.player.pathing.coord;
                c0.y() == *level
                    && (c0.x() as i32 - *x as i32).abs() <= *radius as i32
                    && (c0.z() as i32 - *z as i32).abs() <= *radius as i32
            }
            Condition::Timeout => {
                env.clock().saturating_sub(self.base.start_tick) >= self.task.budget_ticks as u64
            }
            Condition::Death => active.player.stats.levels[3] == 0,
            Condition::All(cs) => cs.iter().all(|c| self.eval(c, env, pid)),
            Condition::Any(cs) => cs.iter().any(|c| self.eval(c, env, pid)),
            Condition::Not(c) => !self.eval(c, env, pid),
        }
    }
}
