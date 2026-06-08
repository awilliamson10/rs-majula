use crate::active_player::EnginePlayer;
use crate::clients::client_db::DbRequest;
use crate::clients::client_ether::EtherOutbound;
use crate::engine::Engine;
use crate::player_save::{extract_profile, save_binary};
use rs_vm::engine::ScriptPlayer;
use rs_vm::pointer::ScriptPointer;
use rs_vm::state::{QueuePriority, ScriptArgument, ScriptState};
use rs_vm::subject::ScriptSubject;
use rs_vm::trigger::ServerTriggerType;
use tracing::{error, info};

impl Engine {
    /// Processes the logout phase of the engine tick cycle.
    ///
    /// Iterates over every active player and handles pending disconnection
    /// or voluntary logout:
    ///
    /// 1. Checks the player's `disconnect_rx` channel and sets
    ///    `logout_requested` if a disconnect signal is received.
    /// 2. If a logout is already in progress (`logout_sent`), the player is
    ///    marked for removal.
    /// 3. If a logout is requested but temporarily prevented (e.g. in
    ///    combat), the prevention message is shown and the request is
    ///    cleared.
    /// 4. Otherwise, calls `active.logout()` to begin the logout sequence.
    ///
    /// For each player marked for removal, the engine:
    ///
    /// * Closes any open modal interface.
    /// * Checks whether the player's queues allow immediate removal.
    /// * Executes the `Logout` server trigger script.
    /// * Persists the player profile via [`DbRequest::Save`].
    /// * Notifies the ether service with [`EtherOutbound::PlayerLogout`].
    /// * Removes the player from the engine.
    ///
    /// # Side Effects
    ///
    /// * Sends save requests to the database.
    /// * Sends logout notifications to the ether service.
    /// * Removes player slots from `self.players` and `self.active_players`.
    /// * Executes the `Logout` RuneScript trigger.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `Engine::cycle`
    /// **Calls:** `remove_player`, `runescript_vm_execute`, `extract_profile`,
    ///   `save_binary`
    pub(crate) fn logouts(&mut self) {
        let mut removals = Vec::new();

        for &pid in self.player_list.processing.iter() {
            let Some(active) = self.player_list.players[pid as usize].as_mut() else {
                continue;
            };

            if !active.player.logout_requested && active.handle.disconnect_rx.try_recv().is_ok() {
                active.player.logout_requested = true;
            }
            if active.player.logout_sent {
                removals.push(pid);
            } else if active.player.logout_requested {
                if let Some(until) = active.player.logout_prevented_until
                    && self.clock < until
                {
                    if let Some(msg) = active.player.logout_prevented_message.take() {
                        active.message_game(&msg);
                    }
                    active.player.logout_requested = false;
                    continue;
                }
                active.logout();
            }
        }

        if removals.is_empty() {
            return;
        }

        for &index in &removals {
            let idx = index as usize;

            let (uid, user37, can_access, queue_discard, no_head) = {
                let Some(active) = &mut self.player_list.players[idx] else {
                    continue;
                };
                if let Err(e) = active.close_modal(true) {
                    error!(
                        "error closing modal during logout for player {}: {e}",
                        active.player.uid.pid()
                    );
                }
                let mut queue_discard = true;
                let mut h = active.player.state.queues.queue.head();
                while let Some(idx) = h {
                    if active.player.state.queues.queue[idx].priority == QueuePriority::Long {
                        if let Some(ScriptArgument::Int(1)) = active.player.state.queues.queue[idx]
                            .args
                            .as_ref()
                            .and_then(|a| a.first())
                        {
                            h = active.player.state.queues.queue.next();
                            continue;
                        }
                    }
                    queue_discard = false;
                    break;
                }
                (
                    active.player.uid,
                    active.uid().username37(),
                    active.can_access(),
                    queue_discard,
                    active.player.state.queues.engine.head().is_none(),
                )
            };

            if can_access && no_head && queue_discard {
                match self.script_by_key(ServerTriggerType::Logout, None, None) {
                    Some(script) => {
                        let mut state =
                            ScriptState::init(script, Some(ScriptSubject::Player(uid)), None, None);
                        state.pointers.add(ScriptPointer::ProtectedActivePlayer);
                        self.runescript_vm_execute(&mut state);

                        if let Some(active) = self.player_list.players[idx].as_ref() {
                            if let Some(tx) = &self.db_tx {
                                let profile = extract_profile(&active.player, self.cache);
                                let binary = save_binary(&profile, self.cache);
                                let _ = tx.send(DbRequest::Save {
                                    user37,
                                    username: uid.username(),
                                    profile: Box::new(profile),
                                    binary,
                                });
                            }
                        }

                        if let Some(tx) = &self.ether_tx {
                            let _ = tx.send(EtherOutbound::PlayerLogout { user37 });
                        }
                        self.remove_player(index);
                        info!(
                            "Player '{}' disconnected, removed from uid={:?}, pid={}",
                            uid.username(),
                            uid,
                            uid.pid()
                        );
                    }
                    None => error!("LOGOUT TRIGGER IS BROKEN!"),
                }
            }
        }
    }
}
