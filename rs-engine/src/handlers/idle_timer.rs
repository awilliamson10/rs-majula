use crate::active_player::ActivePlayer;
use crate::handlers::ClientGameHandler;
use rs_protocol::network::game::client::idle_timer::IdleTimer;
use rs_vm::ScriptError;

/// Handles the `IdleTimer` client protocol message.
///
/// Sent by the client when the player has been idle for an extended period.
/// In release builds, this triggers a logout request. In debug builds, the
/// logout is suppressed (the flag is cleared instead) to prevent disconnections
/// during development.
///
/// # Arguments
///
/// * `active` - The active player whose client sent this message.
///
/// # Returns
///
/// * `Ok(())` always.
///
/// # Side Effects
///
/// * In release: sets `player.logout_requested` to `true`.
/// * In debug: sets `player.logout_requested` to `false`.
///
/// # Call Stack
///
/// **Called by:** `ActivePlayer::decode_and_handle` (via `ClientGameHandler` dispatch)
impl ClientGameHandler for IdleTimer {
    fn handle(self, active: &mut ActivePlayer) -> Result<(), ScriptError> {
        #[cfg(debug_assertions)]
        {
            active.player.logout_requested = false;
        }
        #[cfg(not(debug_assertions))]
        {
            active.player.logout_requested = true;
        }

        Ok(())
    }
}
