//! ★ NO ENGINE IS BOOTED. The accessors read the process-global label store, so
//! the host pointer is ignored and these tests are pure. That is also why a
//! NULL host must be safe: nothing dereferences it.
use rs_engine::wheels::{self, HintLabel};

#[test]
fn each_hint_kind_reports_its_own_payload() {
    let h = std::ptr::null_mut();

    wheels::record_hint(HintLabel::Npc(42));
    assert_eq!(rs_host::host_hint_kind(h), 1);
    assert_eq!(rs_host::host_hint_a(h), 42);
    assert_eq!(rs_host::host_hint_b(h), -1, "an npc hint has no second coordinate");

    wheels::record_hint(HintLabel::Tile { x: 3222, z: 3218 });
    assert_eq!(rs_host::host_hint_kind(h), 2);
    assert_eq!(rs_host::host_hint_a(h), 3222);
    assert_eq!(rs_host::host_hint_b(h), 3218);

    wheels::record_hint(HintLabel::None);
    assert_eq!(rs_host::host_hint_kind(h), 0);
    assert_eq!(rs_host::host_hint_a(h), -1);
}

#[test]
fn the_flash_tab_reports_minus_one_when_there_is_none() {
    let h = std::ptr::null_mut();
    wheels::record_flash_tab(None);
    assert_eq!(rs_host::host_flash_tab(h), -1);
    wheels::record_flash_tab(Some(3));
    assert_eq!(rs_host::host_flash_tab(h), 3);
}

#[test]
fn the_suppression_switch_is_reachable_over_the_abi() {
    let h = std::ptr::null_mut();
    rs_host::host_wheels_suppress(h, 1);
    assert!(wheels::suppressed());
    rs_host::host_wheels_suppress(h, 0);
    assert!(!wheels::suppressed());
}
