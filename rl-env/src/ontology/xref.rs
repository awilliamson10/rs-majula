//! A cross-reference over the content source: what defines each constant, and
//! which scripts read or write each var.
//!
//! ★★ A `.constant` FILE IS NOT A COMPLETE LIST OF A VAR'S STATES. Bare
//! literals are common -- `%cookquest = 1` (`quest_cook.rs2:32`) is one, and
//! `quest_cook` has no `.constant` file at all. Harvest literals from `.rs2`
//! too or the extractor silently omits states.
//!
//! ★★ AND IT IS NOT A LIST OF REACHABLE STATES EITHER. `%tutorial = 3`
//! (`tutorial.rs2:145`) is guarded by `if (%tutorial = 2)`, and nothing in the
//! tree ever assigns 2 -- so 3 is named nowhere AND reachable never. This pass
//! answers "what does the source assign"; reachability is a question for the
//! runtime journal, not for a text scan. Do not conflate them.
//!
//! ★ Deliberately a line scanner, not a RuneScript parser. It over-reports (a
//! `%var` inside a comment counts as a read) rather than under-reports, because
//! a missed state is a silently-zero-scoring task while a spurious read shows
//! up immediately when a human reads the curated layer.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constant {
    pub name: String,
    pub value: i32,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarpWrite {
    pub literal: Option<i32>,
    pub constant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarpRef {
    pub varp: String,
    pub file: String,
    pub line: u32,
    pub write: Option<VarpWrite>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Xref {
    pub constants: BTreeMap<String, Constant>,
    pub varp_refs: Vec<VarpRef>,
}

impl Xref {
    /// Every integer value assigned to `var` anywhere in the tree, with
    /// `^constant` writes resolved through `self.constants`. A write naming a
    /// constant that is defined nowhere is dropped here -- and that is worth
    /// noticing, so the report surfaces it separately.
    pub fn values_assigned_to(&self, var: &str) -> BTreeSet<i32> {
        let mut out = BTreeSet::new();
        for r in self.varp_refs.iter().filter(|r| r.varp == var) {
            let Some(w) = &r.write else { continue };
            if let Some(v) = w.literal {
                out.insert(v);
            }
            if let Some(name) = &w.constant {
                if let Some(c) = self.constants.get(name) {
                    out.insert(c.value);
                }
            }
        }
        out
    }
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

/// `^name = 42`
fn parse_constant(line: &str) -> Option<(String, i32)> {
    let rest = line.trim().strip_prefix('^')?;
    let (name, value) = rest.split_once('=')?;
    Some((name.trim().to_string(), value.trim().parse().ok()?))
}

/// Every `%var` on a line, with the write (if any) that line performs.
///
/// ★★ RuneScript USES A SINGLE `=` FOR BOTH ASSIGNMENT AND COMPARISON
/// (`if (%tutorial = ^x)`), so the two are told apart by whether the `%var`
/// OPENS its statement. `cook_journal.rs2` compares `%cookquest` on every
/// branch and writes it never; misclassifying those as writes invents three
/// states that do not exist.
///
/// ★★ AND MULTIPLE STATEMENTS SHARE A LINE. `tutorial.rs2:144` is
/// `if (%tutorial = 2) {` with `%tutorial = 3;` on the next line, but the
/// one-line form occurs too. Splitting on `{ } ;` first is what lets a trailing
/// assignment be seen at all -- a parser that inspects only the start of the
/// LINE loses it silently, and 3 is the one state `.constant` does not name.
///
/// ★ A continuation line of a multi-line condition opens with `|` or `&`
/// (`tut_mining.rs2:113`: `| %tutorial = 290`), so it does not open with the
/// var and is correctly read as a comparison.
fn parse_refs(line: &str, file: &str, lineno: u32) -> Vec<VarpRef> {
    let mut out = Vec::new();
    for stmt in line.split(['{', '}', ';']) {
        let trimmed = stmt.trim();
        let mut i = 0usize;
        while let Some(pos) = stmt[i..].find('%') {
            let start = i + pos + 1;
            let end = start
                + stmt[start..]
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(stmt.len() - start);
            if end == start {
                i = start;
                continue;
            }
            let var = stmt[start..end].to_string();
            let opens = trimmed
                .strip_prefix('%')
                .is_some_and(|t| t.starts_with(&var));
            let write = if opens {
                stmt[end..].trim_start().strip_prefix('=').map(|rhs| {
                    let rhs = rhs.trim();
                    match rhs.strip_prefix('^') {
                        Some(c) => VarpWrite {
                            literal: None,
                            constant: c.trim().split_whitespace().next().map(str::to_string),
                        },
                        None => VarpWrite {
                            literal: rhs.split_whitespace().next().and_then(|t| t.parse().ok()),
                            constant: None,
                        },
                    }
                })
            } else {
                None
            };
            out.push(VarpRef { varp: var, file: file.to_string(), line: lineno, write });
            i = end;
        }
    }
    out
}

pub fn scan(root: &Path) -> Xref {
    let mut x = Xref::default();

    let mut constant_files = Vec::new();
    walk(root, "constant", &mut constant_files);
    constant_files.sort();
    for path in &constant_files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let file = path.to_string_lossy().to_string();
        for (n, line) in text.lines().enumerate() {
            if let Some((name, value)) = parse_constant(line) {
                x.constants.entry(name.clone()).or_insert(Constant {
                    name,
                    value,
                    file: file.clone(),
                    line: n as u32 + 1,
                });
            }
        }
    }

    let mut script_files = Vec::new();
    walk(root, "rs2", &mut script_files);
    script_files.sort();
    for path in &script_files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let file = path.to_string_lossy().to_string();
        for (n, line) in text.lines().enumerate() {
            x.varp_refs.extend(parse_refs(line, &file, n as u32 + 1));
        }
    }

    x
}
