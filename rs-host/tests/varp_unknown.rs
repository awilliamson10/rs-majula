//! `host_varp` must never panic.
//!
//! ★ ONE ENGINE PER PROCESS: this file calls `host_new` exactly once, and it
//! must stay the only `host_new` in this test binary.

use std::ffi::CString;

/// Mirrors `rs_host::HOST_VARP_UNKNOWN`. Duplicated deliberately: a test that
/// imported the constant would pass even if the constant changed, and the
/// value the TypeScript side hardcodes is a literal, not an import.
const HOST_VARP_UNKNOWN: i32 = i32::MIN;

#[test]
fn an_unknown_varp_name_returns_a_sentinel_instead_of_aborting_the_process() {
    let h = rs_host::host_new(4242);

    // A real varp still reads normally — otherwise the sentinel could be
    // "every name is unknown" and this test would pass on a broken lookup.
    let known = CString::new("tutorial").unwrap();
    let v = rs_host::host_varp(h, known.as_ptr());
    assert_eq!(v, 0, "a fresh Tutorial Island spawn must be at %tutorial = 0");

    // ★ Before the fix this line ABORTS the test process — no panic message,
    // no failure report, just a killed binary. That is exactly the failure
    // mode a manager composing varp names would hit in production.
    let bogus = CString::new("definitely_not_a_varp").unwrap();
    assert_eq!(rs_host::host_varp(h, bogus.as_ptr()), HOST_VARP_UNKNOWN);

    // A null name must not be dereferenced either.
    assert_eq!(rs_host::host_varp(h, std::ptr::null()), HOST_VARP_UNKNOWN);

    rs_host::host_free(h);
}
