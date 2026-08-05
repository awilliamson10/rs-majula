//! The AGGRESSION selectors: which entity an npc is engaging, read off fields the
//! engine actually writes.
//!
//! ★★ WHY THIS FILE EXISTS. `host_npc_field`'s field 4 reads `Npc::target_player`,
//! and G1c measured `target_player >= 0` in 0 of 1,713,343 npc rows — every
//! process, every segment, every tick — in a corpus that carried 343
//! player-damage events, 1,355 attack swings and 57 npc kills. The field is
//! declared in `rs-entity` and assigned NOWHERE in the workspace. A label column
//! that is constant is not a hard variable to learn; it is not a variable at all,
//! and a probe scored on it reports either "undefined" or "100%" and means
//! neither. Fields 10-14 replace it with `Npc::hunt_target` and
//! `Npc::interaction`, which the engine does write.
//!
//! ★ ONE ENGINE PER PROCESS: this binary calls the constructor exactly once and
//! therefore holds exactly ONE `#[test]` fn. `rs-pathfinder` keeps
//! `COLLISION_FLAGS` process-global and `host_new_at` asserts on a second call,
//! so splitting the assertions below across test fns would ABORT the binary
//! rather than run them.

/// ★ A LITERAL, not `rs_host::HOST_FIELD_UNKNOWN`. A test that imported the
/// constant would keep passing if the constant itself changed to something a real
/// field can hold. Same reason `tests/varp_unknown.rs` and
/// `tests/truth_accessors.rs` duplicate their sentinels.
const HOST_FIELD_UNKNOWN: i64 = i64::MIN;

/// Lumbridge, the mainland respawn: densely populated, so the loop below is not
/// vacuous.
const LUMBRIDGE: (u16, u8, u16) = (3222, 0, 3218);

/// Field selectors, mirrored from `host_npc_field`'s doc-comment table rather
/// than imported — a test that imported them would pass even if a selector's
/// MEANING changed underneath the name.
///
/// ★★ THESE WERE READ OFF `rs-host/src/lib.rs`, NOT GUESSED. The plan that
/// commissioned this file proposed 8 and 9 for the two new fields; 8 and 9 were
/// already `typeId` and `active`, both live. A wrong selector reads a DIFFERENT
/// REAL FIELD and returns entirely plausible numbers, so nothing downstream can
/// detect it — which is what `assert_kinds_are_kinds` below is for.
const F_TARGET_PLAYER: u32 = 4;
const F_HUNT_TARGET_KIND: u32 = 10;
const F_HUNT_TARGET_INDEX: u32 = 11;
const F_INTERACTION_KIND: u32 = 12;
const F_INTERACTION_INDEX: u32 = 13;
const F_INTERACTION_OP: u32 = 14;

/// `INTERACTION_KIND_*`, mirrored. -1 none, 0 obj, 1 loc, 2 npc, 3 player.
const KINDS: [i64; 5] = [-1, 0, 1, 2, 3];

/// The highest `NpcMode` discriminant (`Queue20`, `rs-pack/src/types.rs:436+`).
const MAX_NPC_MODE: i64 = 66;

#[test]
fn the_aggression_fields_are_readable_encoded_and_never_panic() {
    let (x, level, z) = LUMBRIDGE;
    let h = rs_host::host_new_at(4242, x, level, z);
    assert!(!h.is_null());

    for _ in 0..40 {
        rs_host::host_step(h);
    }

    let n = rs_host::host_npc_count(h);
    assert!(
        n > 0,
        "no npcs in range of a mainland spawn — every assertion below would be \
         vacuously true and the selectors would be untested"
    );

    let mut ops = Vec::new();
    let mut hunt_kinds = Vec::new();
    let mut interaction_kinds = Vec::new();

    for slot in 0..n {
        let f = |field| rs_host::host_npc_field(h, slot, field);

        // -- readable at all -------------------------------------------------------
        //
        // ★ Before the selectors existed these five fell through to the `_` arm
        // and returned the sentinel, which is what made this test fail.
        for field in [
            F_HUNT_TARGET_KIND,
            F_HUNT_TARGET_INDEX,
            F_INTERACTION_KIND,
            F_INTERACTION_INDEX,
            F_INTERACTION_OP,
        ] {
            assert_ne!(
                f(field),
                HOST_FIELD_UNKNOWN,
                "field {field} is unhandled for slot {slot}: it would land in a label \
                 file as -2^63, i.e. a colossal outlier where an error belongs"
            );
        }

        let (hk, hi) = (f(F_HUNT_TARGET_KIND), f(F_HUNT_TARGET_INDEX));
        let (ik, ii) = (f(F_INTERACTION_KIND), f(F_INTERACTION_INDEX));
        let op = f(F_INTERACTION_OP);

        // -- the encoding is the encoding -----------------------------------------
        //
        // ★★ THIS IS THE SELECTOR-NUMBER CHECK. A kind selector pointed at the
        // wrong field reads something real — a tile (3222), a type id (1234), a
        // hitpoint count — and every one of those is outside {-1,0,1,2,3}. It is
        // the only cheap defence against silently reading the wrong column.
        assert!(KINDS.contains(&hk), "slot {slot}: hunt kind {hk} is not a kind");
        assert!(KINDS.contains(&ik), "slot {slot}: interaction kind {ik} is not a kind");

        // ★ The pair is an `Option`: both halves are absent together or present
        // together. A kind that says "player" beside an index of -1 would be an
        // unjoinable row, and a -1 kind beside a real index would be a target with
        // no numbering scheme to read it in.
        assert_eq!(
            hk == -1,
            hi == -1,
            "slot {slot}: hunt kind {hk} and index {hi} disagree about absence"
        );
        assert_eq!(
            ik == -1,
            ii == -1,
            "slot {slot}: interaction kind {ik} and index {ii} disagree about absence"
        );

        // ★ An `NpcMode` discriminant or -1 — never a coordinate, never a pid.
        assert!(
            op == -1 || (0..=MAX_NPC_MODE).contains(&op),
            "slot {slot}: op {op} is not an NpcMode discriminant"
        );

        // -- the dead field stays exposed, and stays dead ---------------------------
        //
        // ★ NOT redundant with G1c's corpus measurement. It is the control: if this
        // ever stops being -1, the engine grew a writer for `target_player` and the
        // whole premise of fields 10-14 needs revisiting.
        assert_eq!(
            f(F_TARGET_PLAYER),
            -1,
            "slot {slot}: `Npc::target_player` is assigned nowhere in the workspace, \
             so a non--1 here means the engine changed under this task"
        );

        ops.push(op);
        hunt_kinds.push(hk);
        interaction_kinds.push(ik);
    }

    // -- the fields are LIVE, not merely well-typed --------------------------------
    //
    // ★★ THE NON-VACUITY GATE, and the thing that separates this from
    // `target_player`. Every assertion above passes for a field the engine never
    // writes: `None` encodes as -1, -1 is a valid kind, and -1 == -1 satisfies the
    // pair check. `target_op` is the one field guaranteed to be live after a tick
    // — `npc_process_movement_interaction`'s failsafe writes `default_mode` into
    // it whenever both the op and the target are None (`phases/npc.rs:1107-1109`)
    // — so an all--1 op column means selector 14 is pointed at something dead.
    assert!(
        ops.iter().any(|&o| o >= 0),
        "every npc reads op -1 after 40 ticks. The engine's own failsafe sets \
         `target_op` to the npc's `default_mode`, so this means field 14 is \
         reading a field nothing writes — exactly the `target_player` failure \
         this task exists to fix. ops = {ops:?}"
    );

    // ★ Printed, not asserted. `--nocapture` turns this into the observed-values
    // evidence the task report needs, without pinning a distribution that depends
    // on which Lumbridge npcs happened to be in range.
    println!("slots={n} ops={ops:?}");
    println!("hunt_kinds={hunt_kinds:?}");
    println!("interaction_kinds={interaction_kinds:?}");

    // -- the sentinel still means what it means -------------------------------------
    //
    // ★ A panic in an `extern "C"` fn ABORTS the process — no unwinding across a C
    // frame means no JS-visible error, just a dead host. These read a slot that
    // does not exist and a field id that does not exist for EVERY new selector.
    for field in [
        F_HUNT_TARGET_KIND,
        F_HUNT_TARGET_INDEX,
        F_INTERACTION_KIND,
        F_INTERACTION_INDEX,
        F_INTERACTION_OP,
    ] {
        assert_eq!(
            rs_host::host_npc_field(h, 99_999, field),
            HOST_FIELD_UNKNOWN,
            "field {field} on a nonexistent slot must be the sentinel"
        );
    }
    // ★ One past the last real selector: the `_` arm must still be reachable, or a
    // typo'd field id would silently return field 14's value.
    assert_eq!(rs_host::host_npc_field(h, 0, 15), HOST_FIELD_UNKNOWN);
    assert_eq!(rs_host::host_npc_field(h, 0, 99), HOST_FIELD_UNKNOWN);

    rs_host::host_free(h);
}
