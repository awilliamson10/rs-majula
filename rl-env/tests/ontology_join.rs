//! Task 5: what the packer does NOT already guarantee.
//!
//! ★★ CONFIG STANZAS <-> PACK REGISTRY IS ALREADY A HARD INVARIANT, ENFORCED AT
//! PACK TIME, ON EVERY BOOT. `rs-pack/src/pack/config/varp.rs` walks
//! `0..pack.max`, panics via `pack.get_by_id` on an id gap and via
//! `configs.get(debugname)` on a name with no stanza, then COPIES the name into
//! the cache. So a registry<->cache diff is tautological and a config<->registry
//! diff can never fire. An earlier draft of this plan specified exactly that
//! diff, with a five-entry allowlist; both halves were wrong.
//!
//! What is left worth checking is reachability and resolution.

use rl_env::ontology;

#[test]
fn every_source_var_reference_resolves_to_a_varp_or_a_varbit() {
    let o = ontology::build();
    let r = ontology::report(&o);
    assert!(
        r.unresolved_refs.is_empty(),
        "{} `%name` references resolve to neither a varp nor a varbit:\n{}",
        r.unresolved_refs.len(),
        r.unresolved_refs.join("\n"),
    );
    // ★ The varbit half must be non-empty, or the varbit lookup is not actually
    // being consulted and the assertion above is passing vacuously.
    assert!(
        !r.varbit_refs.is_empty(),
        "no varbit references found -- is the varbit dump wired up?",
    );
}

#[test]
fn every_constant_a_var_write_names_is_resolvable() {
    // ★ An unresolvable `^constant` silently drops a state from
    // `values_assigned_to`. Each failure is either a scanner gap or a constant
    // defined somewhere the walk does not reach. Never suppress one to go green.
    let o = ontology::build();
    let r = ontology::report(&o);
    assert!(
        r.unresolved_constants.is_empty(),
        "{} unresolved constants:\n{}",
        r.unresolved_constants.len(),
        r.unresolved_constants.join("\n"),
    );
}

#[test]
fn unreferenced_varps_are_reported_but_do_not_fail() {
    // ★ NOT an assertion. A cache varp no script mentions is a REACHABILITY
    // observation, not an error -- 141 of 359 varps come from the five
    // `_unpack/{225,244,245,254,274}` rev dumps inside the rev-274 tree and
    // many are genuinely dead here. The number belongs in the artifact's diff,
    // not in a pass/fail gate.
    let o = ontology::build();
    let r = ontology::report(&o);
    println!("varps referenced by no script: {}", r.unreferenced.len());
}
