//! ★★ TUTORIAL ISLAND'S TRAINING WHEELS, TURNED INTO A LABEL CHANNEL.
//!
//! The island marks its own targets — a yellow arrow over the npc, a flashing
//! tab icon, a chatbox panel with the step's text. Rendered, they teach a
//! policy to click the flashing thing, which scores beautifully here and is
//! worthless off the island. So they are not SENT; they are RECORDED, and the
//! privileged teacher reads them as labels. Faithfulness constrains the policy,
//! not the labels.
//!
//! ★ PROCESS-GLOBAL, deliberately. ONE ENGINE PER PROCESS is already an
//! invariant of this system (`rs-pathfinder`'s COLLISION_FLAGS is global for
//! the same reason), so a global here adds no new constraint.
//!
//! ★★ DEFAULT OFF. `make tape` runs `packet_tape` with no spawn, which IS
//! Tutorial Island — suppressing by default changes the tape, `feed_bytes`
//! stops being 2208, and `live.test.ts`'s pins go with it.
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
static HINT: Mutex<HintLabel> = Mutex::new(HintLabel::None);
static TUT_COM: Mutex<Option<u16>> = Mutex::new(None);
static FLASH_TAB: Mutex<Option<u8>> = Mutex::new(None);

pub fn set_suppressed(on: bool) { SUPPRESS.store(on, Ordering::Relaxed); }
pub fn suppressed() -> bool { SUPPRESS.load(Ordering::Relaxed) }

/// ★ A poisoned lock must not take the engine with it — every caller is on a
/// packet path. `unwrap_or_else(|e| e.into_inner())` keeps the value.
pub fn record_hint(h: HintLabel) { *HINT.lock().unwrap_or_else(|e| e.into_inner()) = h; }
pub fn hint() -> HintLabel { *HINT.lock().unwrap_or_else(|e| e.into_inner()) }

pub fn record_tut_com(c: Option<u16>) { *TUT_COM.lock().unwrap_or_else(|e| e.into_inner()) = c; }
pub fn tut_com() -> Option<u16> { *TUT_COM.lock().unwrap_or_else(|e| e.into_inner()) }

pub fn record_flash_tab(t: Option<u8>) { *FLASH_TAB.lock().unwrap_or_else(|e| e.into_inner()) = t; }

/// # ★★ MONOTONE BY DESIGN, NOT BY OMISSION: THIS IS THE LAST TAB EVER
/// FLASHED, NOT "IS ONE FLASHING RIGHT NOW"
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
/// the engine last ask to flash. `Some(id)` after the run's last `tut_flash`
/// call means exactly that and nothing about whether the client is still
/// blinking it -- a consumer that needs "flashing now" has no channel here to
/// read it from, engine-side or client-side truth alike.
pub fn flash_tab() -> Option<u8> { *FLASH_TAB.lock().unwrap_or_else(|e| e.into_inner()) }
