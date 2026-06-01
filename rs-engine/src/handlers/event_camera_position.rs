use crate::active_player::ActivePlayer;
use crate::handlers::ClientGameHandler;
use rs_protocol::network::game::client::event_camera_position::EventCameraPosition;
use rs_vm::ScriptError;

/// Handles the `EventCameraPosition` client protocol message.
///
/// No-op. The client periodically sends camera position updates, but the server
/// does not currently use this information. The packet is accepted and discarded.
///
/// # Arguments
///
/// * `_` - The active player (unused).
///
/// # Returns
///
/// * `Ok(())` always.
///
/// # Call Stack
///
/// **Called by:** `ActivePlayer::decode_and_handle` (via `ClientGameHandler` dispatch)
impl ClientGameHandler for EventCameraPosition {
    fn handle(self, _: &mut ActivePlayer) -> Result<(), ScriptError> {
        Ok(())
    }
}
