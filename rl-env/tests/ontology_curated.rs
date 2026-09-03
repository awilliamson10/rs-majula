//! Task 10: the curated layer is the small hand-written meaning overlay. Its
//! only mechanical guarantee is that it never describes something that does
//! not exist, and never claims a state no script assigns.

use rl_env::ontology;
use std::path::Path;

#[test]
fn every_curated_entry_describes_a_real_varp_and_real_states() {
    let o = ontology::build();
    let curated = ontology::load_curated(Path::new("ontology/curated"));
    assert!(!curated.is_empty(), "no curated entries found");

    for c in &curated {
        let exists = o.entities.iter()
            .any(|e| e.kind == ontology::EntityKind::Varp && e.name == c.varp);
        assert!(exists, "curated entry describes varp {:?}, which does not exist", c.varp);

        let assigned = o.xref.values_assigned_to(&c.varp);
        for state in c.states.keys() {
            assert!(
                assigned.contains(state),
                "curated {:?} documents state {state}, which no script assigns",
                c.varp,
            );
        }
    }
}

#[test]
fn the_tutorial_and_cookquest_entries_are_present() {
    let curated = ontology::load_curated(Path::new("ontology/curated"));
    let names: Vec<&str> = curated.iter().map(|c| c.varp.as_str()).collect();
    assert!(names.contains(&"tutorial"));
    assert!(names.contains(&"cookquest"));
}
