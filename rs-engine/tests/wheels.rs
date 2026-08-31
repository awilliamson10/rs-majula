//! ★★ The wheels are the LABEL CHANNEL, not deleted state. Suppression must
//! change what reaches the SOCKET and nothing else — every engine-side effect
//! (modal_tutorial tracking, IfClose triggers, %tutorial gating) still runs.
use rs_engine::wheels::{self, HintLabel};

#[test]
fn suppression_defaults_to_off() {
    assert!(!wheels::suppressed(), "default must be off or make tape stops printing 2208");
}

#[test]
fn a_recorded_hint_is_readable_and_overwritten_by_the_next() {
    wheels::record_hint(HintLabel::Npc(42));
    assert_eq!(wheels::hint(), HintLabel::Npc(42));
    wheels::record_hint(HintLabel::Tile { x: 3222, z: 3218 });
    assert_eq!(wheels::hint(), HintLabel::Tile { x: 3222, z: 3218 });
    wheels::record_hint(HintLabel::None);
    assert_eq!(wheels::hint(), HintLabel::None);
}

#[test]
fn the_toggle_round_trips() {
    wheels::set_suppressed(true);
    assert!(wheels::suppressed());
    wheels::set_suppressed(false);
    assert!(!wheels::suppressed());
}
