//! Task 4: the C ABI over the evaluator. These run in-process against the
//! real exports -- `host_new` aborts on a SECOND call, so this file boots
//! exactly one host and every test shares it through a serialising mutex.

use std::ffi::CString;
use std::sync::{Mutex, OnceLock};

static HOST: OnceLock<Mutex<usize>> = OnceLock::new();

/// One host for the whole test binary. `host_new` aborts on a second call
/// (`rs-pathfinder`'s COLLISION_FLAGS is process-global), and cargo runs a
/// test binary's tests on threads within one process.
fn host() -> std::sync::MutexGuard<'static, usize> {
    HOST.get_or_init(|| Mutex::new(rs_host::host_new(4242) as usize))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn task_path() -> CString {
    // rs-host's tests run with CWD = majula/rs-host.
    CString::new("../rl-env/tasks/tutorial_survival.ron").unwrap()
}

#[test]
fn loading_the_tutorial_task_reports_its_milestone_count() {
    let h = *host();
    let n = rs_host::host_task_load(h as *mut _, task_path().as_ptr());
    assert!(n > 0, "expected a positive milestone count, got {n}");
    assert!(n <= 64, "the mask carries at most 64 milestones, got {n}");
}

#[test]
fn a_missing_file_is_a_negative_code_and_not_a_panic() {
    let h = *host();
    let bad = CString::new("../rl-env/tasks/does_not_exist.ron").unwrap();
    let n = rs_host::host_task_load(h as *mut _, bad.as_ptr());
    assert!(n < 0, "a missing file must be negative, got {n}");
}

#[test]
fn milestone_names_are_readable_and_stable_across_calls() {
    let h = *host();
    let n = rs_host::host_task_load(h as *mut _, task_path().as_ptr());
    assert!(n > 0);
    let first = unsafe { std::ffi::CStr::from_ptr(rs_host::host_task_milestone_name(h as *mut _, 0)) }
        .to_string_lossy()
        .into_owned();
    assert!(!first.is_empty(), "milestone 0 must have a name");
    let again = unsafe { std::ffi::CStr::from_ptr(rs_host::host_task_milestone_name(h as *mut _, 0)) }
        .to_string_lossy()
        .into_owned();
    assert_eq!(first, again, "the name must not be a pointer to a temporary");
}

/// A one-milestone task whose progress needs no player input at all:
/// `Timeout` becomes true once the engine clock has advanced `budget_ticks`
/// past the tick the task was armed at (see `Armed::eval`'s `Condition::
/// Timeout` arm). With `budget_ticks: 1` and `Engine::cycle` documented to
/// increment `self.clock` by exactly 1, the very first `host_step` after
/// arming must flip this milestone from unset to latched.
///
/// ★ Written to a temp file, not `rl-env/tasks/`: that directory is for real
/// curricula, not a fixture that exists solely to give this test a condition
/// it can force deterministically without a client driving tutorial dialogue.
fn write_timeout_task() -> (std::path::PathBuf, CString) {
    let path = std::env::temp_dir().join(format!(
        "rs_host_task_ffi_timeout_{}.ron",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"Task(
    name: "timeout_after_one_tick",
    budget_ticks: 1,
    budget_turns: 1,
    start: Start(
        at: Coord(3094, 0, 3107),
        seed: 4242,
        jitter: 0,
        loadout: Loadout(stats: [], worn: [], inventory: [], vars: []),
    ),
    progress: [
        Milestone(name: "one_tick_elapsed", when: Timeout),
    ],
    goal: Timeout,
    fail: None,
)
"#,
    )
    .expect("write the temp task file");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    (path, cpath)
}

#[test]
fn stepping_folds_the_task_and_the_mask_is_monotone() {
    let h = *host();
    let (path, cpath) = write_timeout_task();
    let n = rs_host::host_task_load(h as *mut _, cpath.as_ptr());
    let _ = std::fs::remove_file(&path);
    assert_eq!(n, 1, "expected exactly one milestone, got {n}");

    // Freshly armed: nothing has folded yet, so the mask must read 0.
    let before = rs_host::host_task_mask(h as *mut _);
    assert_eq!(before, 0, "a freshly armed task must start at mask 0, got {before:#x}");

    // ONE step is enough for `Timeout` (budget_ticks: 1) to fire: this is
    // the 0 -> non-zero transition the old version of this test could not
    // observe, because it drove 20 bare `host_step`s against a task whose
    // milestones all gate on `%tutorial`, which nothing in this process
    // advances.
    rs_host::host_step(h as *mut _);
    let mut prev = rs_host::host_task_mask(h as *mut _);
    assert_eq!(
        prev, 1,
        "the Timeout milestone must latch on the first step after budget_ticks=1 has elapsed, got {prev:#x}"
    );

    // And it stays latched -- monotone -- for every step after that.
    for _ in 0..19 {
        rs_host::host_step(h as *mut _);
        let now = rs_host::host_task_mask(h as *mut _);
        assert_eq!(now & prev, prev, "the latched mask lost a bit: {prev:#x} -> {now:#x}");
        prev = now;
    }
    assert_eq!(prev, 1, "the one-milestone mask must still read 1 after 20 steps, got {prev:#x}");
}
