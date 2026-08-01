//! `MAJULA_ROOT` makes the content root a RUNTIME decision.
//!
//! ★ No engine boot here — this is a path-resolution test, and booting would
//! make it one of the slowest tests in the tree for no added coverage.
//!
//! ★★ WHY THIS EXISTS. `rl_env::content_root` used to be
//! `env!("CARGO_MANIFEST_DIR")`, which is baked in AT COMPILE TIME. The project's
//! own workflow is "build heavy things on the desktop" (CLAUDE.md), so the
//! `librs_host` cdylib is routinely built on one machine and `dlopen`'d on
//! another — where the compiled-in path names a directory that does not exist and
//! `pack_all` aborts inside an `extern "C"` fn, i.e. the whole process dies with
//! no message at all.

#[test]
fn majula_root_env_var_overrides_the_compiled_in_manifest_dir() {
    let compiled = rl_env::content_root();

    // SAFETY: `cargo test` runs each `#[test]` on its own thread, but this file
    // holds exactly one test and the whole tree is run with `--test-threads=1`
    // (the pathfinder's process-global state forces that anyway), so nothing
    // else is reading the environment concurrently. The value is removed again
    // below.
    unsafe { std::env::set_var("MAJULA_ROOT", "/tmp/somewhere-else") };
    let overridden = rl_env::content_root();
    unsafe { std::env::remove_var("MAJULA_ROOT") };

    assert_eq!(overridden, std::path::PathBuf::from("/tmp/somewhere-else"));
    assert_ne!(
        overridden, compiled,
        "the override must actually differ from the baked-in path, or this test proves nothing"
    );
    assert_eq!(
        rl_env::content_root(),
        compiled,
        "removing the var must restore the default"
    );
}

/// ★ NON-VACUITY on the fallback. Without this, `content_root` could return
/// literally anything when the var is unset and the test above would still pass —
/// it only compares the default against itself. The default is the MAJULA
/// WORKSPACE ROOT (`majula/`), the directory `content/274` actually lives in, and
/// that is the property `pack_all` depends on.
#[test]
fn the_default_root_is_the_directory_content_274_lives_in() {
    // Defensive: if some other test in this binary leaked the var, this one
    // would be checking the override rather than the default.
    unsafe { std::env::remove_var("MAJULA_ROOT") };

    let root = rl_env::content_root();
    assert!(
        root.join("content/274").is_dir(),
        "content_root() returned {root:?}, which holds no content/274 — pack_all would abort"
    );
}
