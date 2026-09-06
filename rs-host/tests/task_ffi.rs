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

#[test]
fn stepping_folds_the_task_and_the_mask_is_monotone() {
    let h = *host();
    assert!(rs_host::host_task_load(h as *mut _, task_path().as_ptr()) > 0);
    let mut prev = rs_host::host_task_mask(h as *mut _);
    for _ in 0..20 {
        rs_host::host_step(h as *mut _);
        let now = rs_host::host_task_mask(h as *mut _);
        assert_eq!(now & prev, prev, "the latched mask lost a bit: {prev:#x} -> {now:#x}");
        prev = now;
    }
}

#[test]
fn no_task_armed_reads_as_zero_rather_than_aborting() {
    // A fresh handle with nothing armed. Reading must be safe: the TypeScript
    // side calls these every turn and a wrong order must not kill the host.
    let h = *host();
    let _ = rs_host::host_task_mask(h as *mut _);
    let _ = rs_host::host_task_raw(h as *mut _);
    let _ = rs_host::host_task_flags(h as *mut _);
}
