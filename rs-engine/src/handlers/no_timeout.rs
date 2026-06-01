use crate::active_player::ActivePlayer;
use crate::handlers::ClientGameHandler;
use rs_protocol::network::game::client::no_timeout::NoTimeout;
use rs_vm::ScriptError;

/// Handles the `NoTimeout` client protocol message.
///
/// No-op keepalive handler. The client periodically sends this packet to
/// indicate the connection is still alive. The server accepts the packet
/// but takes no action; the connection timeout is managed elsewhere based
/// on packet receipt timing.
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
impl ClientGameHandler for NoTimeout {
    fn handle(self, _: &mut ActivePlayer) -> Result<(), ScriptError> {
        Ok(())
    }
}
