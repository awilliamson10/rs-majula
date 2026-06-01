use crate::active_player::ActivePlayer;
use crate::handlers::ClientGameHandler;
use rs_protocol::network::game::client::close_modal::CloseModal;
use rs_vm::ScriptError;

/// Handles the `CloseModal` client protocol message.
///
/// Requests that the player's current modal interface be closed. The close is
/// not executed immediately; instead, the `request_modal_close` flag is set so
/// the engine processes the close at the appropriate point in the game cycle.
/// This deferred behavior is required for correct interaction with PID-based
/// timing (e.g., trade requests arriving on the same tick as the close).
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
/// * Sets `player.request_modal_close` to `true`.
///
/// # Call Stack
///
/// **Called by:** `ActivePlayer::decode_and_handle` (via `ClientGameHandler` dispatch)
impl ClientGameHandler for CloseModal {
    fn handle(self, active: &mut ActivePlayer) -> Result<(), ScriptError> {
        // For whatever reason the modal is not closed directly here.
        // This was tested in osrs by sending close modal and being traded
        // on the same tick. If you have pid, the trade works. If the other
        // active has pid, they get told you are still busy.
        // Another test is to send close modal and open a new interface same
        // tick. In this case the new interface will also end up closed.
        active.player.request_modal_close = true;

        Ok(())
    }
}
