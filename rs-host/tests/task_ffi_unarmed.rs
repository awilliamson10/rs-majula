//! Task 4 fix round 1, finding 1: reading the task accessors on a host that
//! has NEVER armed a task, in its own process.
//!
//! ★ `task_ffi.rs`'s original version of this test shared one process-global
//! host with every other test in that file (`host_new` aborts on a second
//! call in a process, so all of that file's tests share one `Host`). Rust
//! runs a binary's tests alphabetically, so
//! `loading_the_tutorial_task_reports_its_milestone_count` had already armed
//! a real task on that shared host by the time the "unarmed" test ran --
//! `host.task` was `Some(_)`, not `None`, and the test exercised the already-
//! covered armed path instead of the one it was named for. A separate test
//! FILE is a separate binary and therefore a separate process, so `host_new`
//! here boots a host that has never seen `host_task_load` -- genuinely
//! unarmed, with no dependence on test order or file-sort tricks.
#[test]
fn no_task_armed_reads_as_zero_rather_than_aborting() {
    let h = rs_host::host_new(4242);
    assert_eq!(rs_host::host_task_mask(h), 0, "no task armed: mask must read 0");
    assert_eq!(rs_host::host_task_raw(h), 0, "no task armed: raw must read 0");
    assert_eq!(rs_host::host_task_flags(h), 0, "no task armed: flags must read 0");
}
