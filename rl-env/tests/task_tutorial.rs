//! Task 8: the first real task file.

use rl_env::ontology;
use rl_env::task::{Cmp, Condition, Task};

const TUTORIAL_STATES: [i32; 14] = [0, 1, 4, 10, 20, 30, 40, 50, 60, 70, 80, 90, 120, 130];

#[test]
fn the_tutorial_task_loads_and_resolves() {
    let o = ontology::build();
    let t = Task::load("tasks/tutorial_survival.ron").expect("load");
    t.resolve(&o).unwrap_or_else(|e| panic!("unresolved: {e:?}"));
    assert_eq!(t.name, "tutorial_survival");
}

#[test]
fn its_milestones_are_the_thirteen_curriculum_transitions() {
    let t = Task::load("tasks/tutorial_survival.ron").expect("load");
    assert_eq!(t.progress.len(), 13, "expected one milestone per curriculum transition");

    let values: Vec<i32> = t.progress.iter().map(|m| match &m.when {
        Condition::Varp(v, Cmp::Ge, n) if v == "tutorial" => *n,
        other => panic!("milestone {:?} is not a `tutorial >= n` condition: {other:?}", m.name),
    }).collect();

    // A milestone fires on REACHING the next state, so the values are the
    // curriculum's states from the second onward.
    assert_eq!(values, TUTORIAL_STATES[1..].to_vec());
}

#[test]
fn its_goal_is_the_survival_curriculums_end_not_the_whole_island() {
    // ★★ ^tutorial_complete = 1000 is the WHOLE island (quest.constant:1).
    // The teacher demonstrates 0 -> 130 only. A goal of 1000 would be a task
    // nothing in this repo can demonstrate.
    let t = Task::load("tasks/tutorial_survival.ron").expect("load");
    assert!(
        matches!(&t.goal, Condition::Varp(v, Cmp::Ge, 130) if v == "tutorial"),
        "goal was {:?}", t.goal,
    );
}

#[test]
fn every_milestone_value_is_actually_assigned_somewhere_in_the_content() {
    // The ontology's whole purpose: a milestone naming a state no script ever
    // sets would score zero forever.
    let o = ontology::build();
    let t = Task::load("tasks/tutorial_survival.ron").expect("load");
    let assigned = o.xref.values_assigned_to("tutorial");
    for m in &t.progress {
        if let Condition::Varp(_, _, n) = &m.when {
            assert!(assigned.contains(n), "milestone {:?} wants %tutorial = {n}, which no script assigns", m.name);
        }
    }
}
