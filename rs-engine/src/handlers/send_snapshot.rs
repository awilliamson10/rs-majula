use crate::active_player::ActivePlayer;
use crate::handlers::ClientGameHandler;
use rs_protocol::network::game::client::send_snapshot::SendSnapshot;
use rs_vm::ScriptError;

impl ClientGameHandler for SendSnapshot {
    fn handle(self, active: &mut ActivePlayer) -> Result<(), ScriptError> {
        if self.reason > 11 {
            // bad client: reason index is out of range
            return Ok(());
        }

        // TODO handle do something with this but who cares..
        active.message_game("Thank-you, your abuse report has been received");
        Ok(())
    }
}
