//! Task 7: a task file must fail at LOAD time on a name the ontology cannot
//! resolve. The alternative failure is a predicate that never fires,
//! presenting as "the policy never learns this task" -- a model bug that is
//! really a typo.

use rl_env::ontology;
use rl_env::task::{At, Cmp, Condition, Task};

#[test]
fn parses_a_minimal_task() {
    let ron = r#"
Task(
    name: "smoke",
    budget_ticks: 64,
    start: Start(
        at: Coord(3222, 0, 3218),
        seed: 4242,
        jitter: 0,
        loadout: Loadout(stats: [], worn: [], inventory: [], vars: []),
    ),
    progress: [ Milestone(name: "first", when: Varp("tutorial", Eq, 1)) ],
    goal: Varp("tutorial", Eq, 130),
    fail: Some(Timeout),
)
"#;
    let t: Task = ron::from_str(ron).expect("parse");
    assert_eq!(t.name, "smoke");
    assert_eq!(t.budget_ticks, 64);
    assert!(matches!(t.start.at, At::Coord(3222, 0, 3218)));
    assert_eq!(t.progress.len(), 1);
    assert!(matches!(t.goal, Condition::Varp(ref v, Cmp::Eq, 130) if v == "tutorial"));
}

#[test]
fn resolves_every_name_against_the_ontology() {
    let o = ontology::build();
    let t = Task::load("tasks/tutorial_survival.ron").expect("load tutorial task");
    let resolved = t.resolve(&o).unwrap_or_else(|errs| panic!("unresolved: {errs:?}"));
    assert!(resolved.spot.0 > 0 && resolved.spot.2 > 0);
}

#[test]
fn an_unknown_varp_fails_resolution_rather_than_scoring_zero_forever() {
    let o = ontology::build();
    let ron = r#"
Task(
    name: "typo",
    budget_ticks: 8,
    start: Start(
        at: Coord(3222, 0, 3218), seed: 1, jitter: 0,
        loadout: Loadout(stats: [], worn: [], inventory: [], vars: []),
    ),
    progress: [],
    goal: Varp("tutoral", Eq, 130),
    fail: None,
)
"#;
    let t: Task = ron::from_str(ron).expect("parse");
    let errs = t.resolve(&o).expect_err("a misspelled varp must not resolve");
    assert!(errs.iter().any(|e| e.contains("tutoral")), "errors were: {errs:?}");
}

#[test]
fn an_unknown_obj_in_a_condition_also_fails() {
    let o = ontology::build();
    let ron = r#"
Task(
    name: "typo2",
    budget_ticks: 8,
    start: Start(
        at: Coord(3222, 0, 3218), seed: 1, jitter: 0,
        loadout: Loadout(stats: [], worn: [], inventory: [], vars: []),
    ),
    progress: [ Milestone(name: "m", when: Inv("pot_of_flour", Ge, 1)) ],
    goal: Timeout,
    fail: None,
)
"#;
    let t: Task = ron::from_str(ron).expect("parse");
    let errs = t.resolve(&o).expect_err("pot_of_flour is not an obj debugname");
    assert!(errs.iter().any(|e| e.contains("pot_of_flour")), "errors were: {errs:?}");
}

/// ✎ NEW. `%` hides varp-vs-varbit in source, so naming a varbit in a Varp
/// condition is the likeliest authoring mistake there is. It must fail loud,
/// and it must say WHY rather than "does not exist".
#[test]
fn a_varbit_named_as_a_varp_fails_with_a_useful_message() {
    let o = ontology::build();
    let ron = r#"
Task(
    name: "varbit_confusion",
    budget_ticks: 8,
    start: Start(
        at: Coord(3222, 0, 3218), seed: 1, jitter: 0,
        loadout: Loadout(stats: [], worn: [], inventory: [], vars: []),
    ),
    progress: [],
    goal: Varp("horrorquest", Eq, 1),
    fail: None,
)
"#;
    let t: Task = ron::from_str(ron).expect("parse");
    let errs = t.resolve(&o).expect_err("horrorquest is a varbit, not a varp");
    assert!(errs.iter().any(|e| e.contains("is a varbit")), "errors were: {errs:?}");
}

#[test]
fn an_npc_start_resolves_to_a_spawn_coordinate() {
    let o = ontology::build();
    let ron = r#"
Task(
    name: "at_the_cook",
    budget_ticks: 8,
    start: Start(
        at: Npc("cook"), seed: 1, jitter: 0,
        loadout: Loadout(stats: [], worn: [], inventory: [], vars: []),
    ),
    progress: [],
    goal: Varp("cookquest", Eq, 2),
    fail: None,
)
"#;
    let t: Task = ron::from_str(ron).expect("parse");
    let r = t.resolve(&o).unwrap_or_else(|e| panic!("unresolved: {e:?}"));
    assert!(r.spot.0 > 0 && r.spot.2 > 0, "cook spawn resolved to {:?}", r.spot);
}
