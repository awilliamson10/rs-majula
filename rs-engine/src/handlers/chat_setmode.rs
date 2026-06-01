use crate::active_player::ActivePlayer;
use crate::clients::client_ether::EtherOutbound;
use crate::engine::engine;
use crate::handlers::ClientGameHandler;
use rs_entity::{ChatSettingsPrivate, ChatSettingsPublic, ChatSettingsTradeDuel};
use rs_protocol::network::game::client::chat_setmode::ChatSetMode;
use rs_vm::ScriptError;
use rs_vm::engine::ScriptPlayer;

/// Handles the `ChatSetMode` client protocol message.
///
/// Updates the player's chat filter settings for public, private, and trade/duel
/// chat channels. Each setting is decoded from its numeric value into the
/// corresponding enum variant. If a value is unrecognized, the handler returns
/// early without modifying that setting.
///
/// After updating the player's settings, the handler sends a chat filter update
/// packet back to the client and, if an ether (cross-world) connection is active,
/// broadcasts the private chat mode change so other worlds can update friend list
/// online status.
///
/// # Arguments
///
/// * `active` - The active player whose client sent this message.
///
/// # Returns
///
/// * `Ok(())` on success or if an unrecognized setting value is encountered.
///
/// # Side Effects
///
/// * Updates `player.public`, `player.private`, and `player.trade` settings.
/// * Sends a `chat_filter_settings` packet to the client.
/// * Sends an `EtherOutbound::ChatModeUpdate` message to the ether service.
///
/// # Call Stack
///
/// **Called by:** `ActivePlayer::decode_and_handle` (via `ClientGameHandler` dispatch)
/// **Calls:** `ActivePlayer::chat_filter_settings`, `EtherOutbound::ChatModeUpdate`
impl ClientGameHandler for ChatSetMode {
    fn handle(self, active: &mut ActivePlayer) -> Result<(), ScriptError> {
        let public = match self.public {
            0 => ChatSettingsPublic::On,
            1 => ChatSettingsPublic::Friends,
            2 => ChatSettingsPublic::Off,
            3 => ChatSettingsPublic::Hide,
            _ => return Ok(()),
        };
        let private = match self.private {
            0 => ChatSettingsPrivate::On,
            1 => ChatSettingsPrivate::Friends,
            2 => ChatSettingsPrivate::Off,
            _ => return Ok(()),
        };
        let trade = match self.trade {
            0 => ChatSettingsTradeDuel::On,
            1 => ChatSettingsTradeDuel::Friends,
            2 => ChatSettingsTradeDuel::Off,
            _ => return Ok(()),
        };

        active.player.public = public;
        active.player.private = private;
        active.player.trade = trade;
        active.chat_filter_settings(self.public, self.private, self.trade);

        let Some(tx) = &engine().ether_tx else {
            return Ok(());
        };

        let _ = tx.send(EtherOutbound::ChatModeUpdate {
            user37: active.uid().username37(),
            private_mode: self.private,
        });

        Ok(())
    }
}
