//! Task 3: the source cross-reference is the ontology's meaning column.
//!
//! ★★ THE REGRESSION TEST THAT MATTERS IS `tutorial_states`. Verified against
//! the tree at 99a1f5d9: the ONLY bare literal ever assigned to `%tutorial`
//! anywhere in `content/274/scripts` is `3` (`tutorial.rs2:145`), and
//! `tutorial.constant` names neither 2 nor 3. So an extractor that reads only
//! `.constant` files silently omits a live state.
//!
//! ★★ AND THE OTHER HALF: `2` is NEVER assigned. It appears only as the
//! comparison `if (%tutorial = 2)` on the line above. Reporting 2 as assigned
//! would assert something false about the game -- naming and reachability are
//! different questions and this pass answers only the first.

use rl_env::ontology::xref;

fn scan() -> xref::Xref {
    let root = rl_env::content_root().join(rs_pack::CONTENT_DIR);
    xref::scan(&root)
}

#[test]
fn resolves_a_constant_defined_in_another_subtree() {
    let x = scan();
    // ^cook_complete is USED in quests/quest_cook/ but DEFINED in
    // general/configs/quest.constant:12. No amount of reading the quest's own
    // config directory finds it -- which is the point of a global scan.
    let c = x.constants.get("cook_complete").expect("^cook_complete not found");
    assert_eq!(c.value, 2);
    assert!(
        c.file.contains("general/configs/quest.constant"),
        "found in {}",
        c.file,
    );
}

#[test]
fn cookquest_writes_are_found_including_the_bare_literal() {
    let x = scan();
    let vals = x.values_assigned_to("cookquest");
    // 1 is a bare literal (quest_cook.rs2:32) named in no .constant file;
    // 2 arrives via ^cook_complete (:88); 0 from the test-cheat namespace.
    assert!(vals.contains(&1), "missed the bare literal 1: {vals:?}");
    assert!(vals.contains(&2), "missed ^cook_complete: {vals:?}");
    assert!(vals.contains(&0), "missed the cheat reset: {vals:?}");
}

#[test]
fn tutorial_states() {
    let x = scan();
    let vals = x.values_assigned_to("tutorial");
    for named in [1, 4, 10, 20, 30, 40, 50, 60, 70, 80, 90, 120, 130] {
        assert!(vals.contains(&named), "missed named tutorial state {named}: {vals:?}");
    }
    // ★★ 0 IS NOT AN ASSIGNED STATE AND MUST NOT BE ONE. Its only occurrence
    // in the tree is the comparison at tutorial.rs2:49; no script ever writes
    // it. 0 is the DEFAULT value of a fresh account. A task file that lists it
    // as a milestone is asserting the agent achieved being newly created.
    assert!(!vals.contains(&0), "0 is a default, not an assignment: {vals:?}");
    assert!(vals.contains(&3), "missed the unnamed literal 3 (tutorial.rs2:145)");
    assert!(
        !vals.contains(&2),
        "2 is only ever compared (tutorial.rs2:144), never assigned -- the `opens` \
         check is treating a comparison as a write",
    );
}

#[test]
fn readers_are_distinguished_from_writers() {
    let x = scan();
    let refs: Vec<_> = x.varp_refs.iter().filter(|r| r.varp == "cookquest").collect();
    assert!(refs.iter().any(|r| r.write.is_some()), "no writes found for cookquest");
    assert!(refs.iter().any(|r| r.write.is_none()), "no reads found for cookquest");
    // cook_journal.rs2 only ever COMPARES %cookquest; it must not be a writer.
    let journal_writes = refs
        .iter()
        .filter(|r| r.file.contains("cook_journal.rs2") && r.write.is_some())
        .count();
    assert_eq!(journal_writes, 0, "cook_journal.rs2 misclassified as a writer");
}

/// ★ A multi-line `if` continues with `| %tutorial = 290` on its own line
/// (`tut_mining.rs2:113`). That leading `|` means the statement does not open
/// with the varp, so it is a comparison -- if the parser gets this wrong it
/// invents a dozen mining states.
#[test]
fn a_continuation_line_of_a_multiline_condition_is_not_a_write() {
    let x = scan();
    // ★ Assert the SPECIFIC line, not the absence of its value: 290 is also
    // assigned legitimately at tut_mining.rs2:97 via its constant, so a
    // set-membership test cannot tell a comparison from a write.
    let r = x
        .varp_refs
        .iter()
        .find(|r| r.varp == "tutorial" && r.file.ends_with("tut_mining.rs2") && r.line == 113)
        .expect("no %tutorial reference recorded at tut_mining.rs2:113");
    assert!(r.write.is_none(), "a leading-pipe continuation was read as a write: {r:?}");
}

/// ★ `%` spells a varbit exactly like a varp. The scanner records the
/// reference; deciding which it is belongs to the join, against the varbit
/// half of the cache dump.
#[test]
fn a_varbit_reference_is_recorded() {
    let x = scan();
    assert!(
        x.varp_refs.iter().any(|r| r.varp == "horrorquest"),
        "horrorquest is referenced in horror_journal.rs2 and was not seen",
    );
}
