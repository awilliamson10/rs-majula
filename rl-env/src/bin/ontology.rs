//! Regenerates the committed ontology artifact.
//!
//! Usage: `cargo run -p rl-env --bin ontology -- <out.json>`
//!
//! ★ Exits NON-ZERO on an unresolvable `%name` or `^constant`, so `make
//! ontology` fails loudly rather than writing a quietly-wrong artifact. A
//! varbit/varn/vars reference and a varp no script mentions are REPORTED, not
//! fatal -- the first is correct (all four share the `%` sigil) and the second
//! is a reachability observation.

fn main() -> std::process::ExitCode {
    let out = std::env::args().nth(1).unwrap_or_else(|| "ontology.json".to_string());
    let o = rl_env::ontology::build();
    let r = rl_env::ontology::report(&o);

    if !r.unresolved_refs.is_empty() || !r.unresolved_constants.is_empty() {
        eprintln!("unresolved `%name` references ({}):", r.unresolved_refs.len());
        for m in &r.unresolved_refs {
            eprintln!("  %{m}");
        }
        eprintln!("unresolved `^constant` writes ({}):", r.unresolved_constants.len());
        for m in &r.unresolved_constants {
            eprintln!("  {m}");
        }
        return std::process::ExitCode::FAILURE;
    }

    // ★ Compact, not pretty. This file is machine-read and committed; pretty
    // printing roughly triples it for no reader benefit, and `git diff` on a
    // generated artifact is read through tooling anyway.
    let json = serde_json::to_string(&o).expect("serialize ontology");
    std::fs::write(&out, &json).unwrap_or_else(|e| panic!("write {out}: {e}"));

    println!(
        "ontology: {} entities, {} constants ({} non-integer), {} var refs, {} npcs with spawns",
        o.entities.len(),
        o.xref.constants.len(),
        o.xref.non_integer_constants.len(),
        o.xref.varp_refs.len(),
        o.spawns.len(),
    );
    println!(
        "  reported: {} varbit/varn/vars refs, {} varps referenced by no script",
        r.varbit_refs.len(),
        r.unreferenced.len(),
    );
    println!("  -> {out} ({} bytes)", json.len());
    std::process::ExitCode::SUCCESS
}
