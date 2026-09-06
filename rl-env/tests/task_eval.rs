//! Task 3: the evaluator. Two properties matter more than coverage of the
//! variants:
//!
//!   1. A milestone true only TRANSIENTLY is latched. This is the whole reason
//!      the fold lives on the tick path rather than at turn boundaries -- an
//!      `Inv` condition goes true and false inside one turn (catch the shrimp,
//!      cook the shrimp), and a turn-boundary scorer reports "the model never
//!      did that step".
//!   2. The latched mask is MONOTONE. A run's score cannot go down.

use rl_env::scenario::Loadout;
use rl_env::task::{At, Cmp, Condition, Milestone, Start, Task};
use rl_env::EnvHarness;
use rs_grid::CoordGrid;

/// A task whose only milestone is "holds a bronze axe", with no goal that can
/// fire. `Timeout` as the goal keeps `budget_ticks` meaningful without ending
/// the test early.
fn axe_task() -> Task {
    Task {
        name: "axe".into(),
        budget_ticks: 100_000,
        budget_turns: 200,
        start: Start {
            at: At::Coord(3222, 0, 3218),
            seed: 4242,
            jitter: 0,
            loadout: Loadout::default(),
        },
        progress: vec![Milestone {
            name: "has_axe".into(),
            when: Condition::Inv("bronze_axe".into(), Cmp::Ge, 1),
        }],
        goal: Condition::Timeout,
        fail: None,
    }
}

#[test]
fn a_milestone_true_only_transiently_is_latched_and_never_unlatches() {
    let mut env = EnvHarness::boot_seeded(4242);
    let lo = Loadout {
        inventory: vec![("bronze_axe".to_string(), 1)],
        ..Loadout::default()
    };
    let pid = env.spawn_and_equip("scorer", CoordGrid::new(3222, 0, 3218), &lo);

    let mut armed = axe_task().arm(&env, pid).expect("arm");
    assert_eq!(armed.latched(), 0, "nothing latched before the first fold");

    armed.fold(&env, pid);
    assert_eq!(armed.latched() & 1, 1, "holding the axe must latch milestone 0");
    assert_eq!(armed.raw() & 1, 1, "and it is true right now");

    // Take the axe away -- the "cook the shrimp" half of the transient case.
    let obj = rl_env::cache()
        .objs
        .get_by_debugname("bronze_axe")
        .expect("bronze_axe is a real obj");
    let inv_id = rl_env::cache().invs.get_by_debugname("inv").expect("inv").id;
    {
        let active = env.engine.get_player_mut(pid).expect("player");
        let inv = active.player.invs.get_mut(&inv_id).expect("backpack");
        inv.delete(obj.id, 1);
    }

    armed.fold(&env, pid);
    assert_eq!(armed.raw() & 1, 0, "the axe is gone, so the raw mask clears");
    assert_eq!(armed.latched() & 1, 1, "but the latch is monotone and holds");
}

#[test]
fn an_unresolvable_name_evaluates_false_rather_than_panicking() {
    let mut env = EnvHarness::boot_seeded(4242);
    let pid = env.spawn_and_equip("scorer", CoordGrid::new(3222, 0, 3218), &Loadout::default());

    let mut t = axe_task();
    t.progress[0].when = Condition::Inv("not_a_real_obj".into(), Cmp::Ge, 1);
    let mut armed = t.arm(&env, pid).expect("arm");
    armed.fold(&env, pid);
    assert_eq!(armed.latched(), 0, "an unknown obj is false, not a panic and not a hit");
}

#[test]
fn a_task_with_more_than_64_milestones_is_refused_at_arm_time() {
    let mut env = EnvHarness::boot_seeded(4242);
    let pid = env.spawn_and_equip("scorer", CoordGrid::new(3222, 0, 3218), &Loadout::default());

    let mut t = axe_task();
    t.progress = (0..65)
        .map(|i| Milestone {
            name: format!("m{i}"),
            when: Condition::Timeout,
        })
        .collect();
    let err = t.arm(&env, pid).expect_err("65 milestones must not arm");
    assert!(err.contains("64"), "the error must name the limit; got {err:?}");
}

#[test]
fn a_varp_condition_reads_engine_truth() {
    let mut env = EnvHarness::boot_seeded(4242);
    let lo = Loadout {
        vars: vec![("tutorial".to_string(), 30)],
        ..Loadout::default()
    };
    let pid = env.spawn_and_equip("scorer", CoordGrid::new(3222, 0, 3218), &lo);

    let mut t = axe_task();
    t.progress[0].when = Condition::Varp("tutorial".into(), Cmp::Ge, 30);
    let mut armed = t.arm(&env, pid).expect("arm");
    armed.fold(&env, pid);
    assert_eq!(armed.latched() & 1, 1, "%tutorial is 30, so >= 30 holds");
}

#[test]
fn the_turn_budget_is_a_separate_stop_from_the_tick_budget() {
    let mut env = EnvHarness::boot_seeded(4242);
    let pid = env.spawn_and_equip("scorer", CoordGrid::new(3222, 0, 3218), &Loadout::default());

    let mut t = axe_task();
    t.budget_turns = 2;
    let mut armed = t.arm(&env, pid).expect("arm");
    assert!(!armed.turns_exhausted());
    armed.note_turn();
    assert!(!armed.turns_exhausted());
    armed.note_turn();
    assert!(armed.turns_exhausted(), "two turns of a two-turn budget is exhausted");
}

#[test]
fn a_departed_player_still_gets_correct_timeout_death_and_composed_answers() {
    let mut env = EnvHarness::boot_seeded(4242);
    let pid = env.spawn_and_equip("scorer", CoordGrid::new(3222, 0, 3218), &Loadout::default());

    // fail: bare Death.  progress[0]: Any([Death, <something false>]) -- the
    // composed case a naive "departed player satisfies Death and nothing
    // else" early return gets wrong, because it never recurses into Any.
    // goal stays Condition::Timeout (from axe_task), with a tiny budget so
    // the clock-driven leg is cheap to drive to completion in the test.
    let mut t = axe_task();
    t.budget_ticks = 5;
    t.fail = Some(Condition::Death);
    t.progress[0].when = Condition::Any(vec![
        Condition::Death,
        Condition::Inv("bronze_axe".into(), Cmp::Ge, 1),
    ]);
    let mut armed = t.arm(&env, pid).expect("arm");

    armed.fold(&env, pid);
    assert!(!armed.failed(), "the player is alive, so Death must not be true yet");
    assert!(!armed.goal(), "budget_ticks=5 has not elapsed yet");

    // Depart the player BEFORE the budget elapses, so a false "not yet timed
    // out" on the next fold proves Timeout is reading the clock rather than
    // returning false because the player is gone.
    env.engine.remove_player(pid);
    armed.fold(&env, pid);
    assert!(armed.failed(), "a departed player IS dead -- fail: Death must fire");
    assert_eq!(
        armed.latched() & 1,
        1,
        "Any([Death, ..]) must recurse into Death for a departed player, not \
         short-circuit false at the Any node"
    );
    assert!(
        !armed.goal(),
        "Timeout must still read the clock for a departed player -- budget_ticks=5 \
         has not elapsed yet, so this must not have flipped true just because the \
         player left"
    );

    for _ in 0..5 {
        env.cycle();
    }
    armed.fold(&env, pid);
    assert!(
        armed.goal(),
        "and once the budget genuinely elapses, Timeout must fire for a departed \
         player exactly as it would for a live one"
    );
}
