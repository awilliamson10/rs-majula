//! ★★ TUTORIAL ISLAND'S TRAINING WHEELS, TURNED INTO A LABEL CHANNEL.
//!
//! The island marks its own targets — a yellow arrow over the npc, a flashing
//! tab icon, a chatbox panel with the step's text. Rendered, they teach a
//! policy to click the flashing thing, which scores beautifully here and is
//! worthless off the island. So they are not SENT; they are RECORDED, and the
//! privileged teacher reads them as labels. Faithfulness constrains the policy,
//! not the labels.
//!
//! ★ WAS PROCESS-GLOBAL; NOW PER-PLAYER, KEYED BY PID. Until this task, the
//! store leaned on ONE ENGINE PER PROCESS (`rs-pathfinder`'s COLLISION_FLAGS is
//! global for the same reason) to justify a single process-global label per
//! engine — with one player in the engine, "the last thing recorded" and
//! "this player's wheels" were the same fact, so a global added no new
//! constraint. That stopped being true the moment one engine could hold more
//! than one active player: a shared global would have let one agent's script
//! clobber every other agent's wheels, and every reader would see whichever
//! pid's script ran last, not the pid it actually asked about. Keying the
//! store by pid is what keeps ONE ENGINE PER PROCESS the only invariant this
//! module leans on now.
//!
//! ★★ DEFAULT OFF. `make tape` runs `packet_tape` with no spawn, which IS
//! Tutorial Island — suppressing by default changes the tape, `feed_bytes`
//! stops being 2208, and `live.test.ts`'s pins go with it.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HintLabel {
    #[default]
    None,
    Npc(u16),
    Tile { x: u16, z: u16 },
    Player(u16),
}

static SUPPRESS: AtomicBool = AtomicBool::new(false);
static HINT: Mutex<Option<HashMap<u16, HintLabel>>> = Mutex::new(None);
static TUT_COM: Mutex<Option<HashMap<u16, u16>>> = Mutex::new(None);
static FLASH_TAB: Mutex<Option<HashMap<u16, u8>>> = Mutex::new(None);

/// ★ SUPPRESS stays a plain, run-wide global. It is a MODE (does the engine
/// draw wheels at all this run), not per-player state -- every player in the
/// engine shares one answer to "is suppression on", unlike the label stores
/// above.
pub fn set_suppressed(on: bool) { SUPPRESS.store(on, Ordering::Relaxed); }
pub fn suppressed() -> bool { SUPPRESS.load(Ordering::Relaxed) }

/// ★ A poisoned lock must not take the engine with it — every caller is on a
/// packet path. `unwrap_or_else(|e| e.into_inner())` keeps the value.
macro_rules! store {
    ($m:expr) => {
        $m.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new)
    };
}

pub fn record_hint(pid: u16, h: HintLabel) { store!(HINT).insert(pid, h); }
pub fn hint(pid: u16) -> HintLabel { store!(HINT).get(&pid).copied().unwrap_or(HintLabel::None) }

pub fn record_tut_com(pid: u16, c: Option<u16>) {
    match c { Some(v) => { store!(TUT_COM).insert(pid, v); } None => { store!(TUT_COM).remove(&pid); } }
}
pub fn tut_com(pid: u16) -> Option<u16> { store!(TUT_COM).get(&pid).copied() }

pub fn record_flash_tab(pid: u16, t: Option<u8>) {
    match t { Some(v) => { store!(FLASH_TAB).insert(pid, v); } None => { store!(FLASH_TAB).remove(&pid); } }
}

/// # ★★ MONOTONE BY DESIGN, NOT BY OMISSION: THIS IS THE LAST TAB EVER
/// FLASHED FOR THIS PLAYER, NOT "IS ONE FLASHING RIGHT NOW"
///
/// `active_player.rs`'s `tut_flash` is the ONLY writer, and every one of its
/// call sites in `content/274` (`tut_chatbox_steps.rs2`) passes a real tab id
/// -- there is no scripted "stop flashing" call anywhere in the tutorial
/// content, and no server packet exists to send one: rev 274's protocol has
/// `TutFlash { tab }` and nothing else in this family. The real client stops
/// blinking a tab PURELY LOCALLY, the moment the player clicks it
/// (`Client.ts:4008` onward reads `tutFlashIcon` but nothing server-side ever
/// tells the client to clear it) -- so "currently flashing" is client-local,
/// ephemeral UI state the engine has no signal for at all, ever. Recording a
/// synthetic `None` here on some other event (closing the tutorial panel, the
/// next hint, ...) would not recover that missing signal; it would invent a
/// clear the real protocol never sends and could read as "the flash
/// stopped" when the client-side blink may still be running. So this
/// answers a different, honestly-answerable question instead: which tab did
/// the engine last ask THIS pid's client to flash. `Some(id)` after this
/// pid's last `tut_flash` call means exactly that and nothing about whether
/// that player's client is still blinking it -- a consumer that needs
/// "flashing now" has no channel here to read it from, engine-side or
/// client-side truth alike. Per-player changes WHOSE map entry this reads,
/// not what the entry means: `forget(pid)` (below) is still the only thing
/// that clears one, and it exists for episode reset, not for "stopped
/// flashing".
pub fn flash_tab(pid: u16) -> Option<u8> { store!(FLASH_TAB).get(&pid).copied() }

/// Drops every wheel recorded for `pid`. Call when a player is removed, so a
/// recycled pid starts clean.
pub fn forget(pid: u16) {
    store!(HINT).remove(&pid);
    store!(TUT_COM).remove(&pid);
    store!(FLASH_TAB).remove(&pid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_are_per_player() {
        forget(41);
        forget(42);
        record_hint(41, HintLabel::Npc(5327));
        record_hint(42, HintLabel::Tile { x: 3098, z: 3107 });
        assert_eq!(hint(41), HintLabel::Npc(5327));
        assert_eq!(hint(42), HintLabel::Tile { x: 3098, z: 3107 });
    }

    #[test]
    fn an_unknown_player_has_no_hint() {
        forget(43);
        assert_eq!(hint(43), HintLabel::None);
        assert_eq!(flash_tab(43), None);
        assert_eq!(tut_com(43), None);
    }

    #[test]
    fn forget_clears_only_that_player() {
        forget(44);
        forget(45);
        record_flash_tab(44, Some(3));
        record_flash_tab(45, Some(7));
        forget(44);
        assert_eq!(flash_tab(44), None, "a respawned pid must not inherit its predecessor's wheels");
        assert_eq!(flash_tab(45), Some(7));
    }
}
