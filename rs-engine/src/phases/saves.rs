use crate::clients::client_db::DbResponse;
use crate::engine::Engine;
use crate::player_save::delete_save_file;
use rs_protocol::LoginResponse;
use tracing::{info, warn};

impl Engine {
    /// Processes the saves phase of the engine tick cycle.
    ///
    /// Drains inbound responses from the database service and handles each
    /// message:
    ///
    /// * `DbReady` -- marks the database as available, enabling logins.
    /// * `DbDisconnected` -- marks the database as unavailable, rejects
    ///   all pending logins with [`LoginResponse::LoginServerOffline`].
    /// * `AuthResponse` -- completes authentication for a pending login.
    ///   On success, marks the login as authenticated and attempts
    ///   completion. On failure, rejects with
    ///   [`LoginResponse::InvalidCredentials`].
    /// * `LoadResponse` -- attaches a loaded player profile to a pending
    ///   login and attempts completion.
    /// * `SaveAck` -- on success, deletes the local backup save file. On
    ///   failure, keeps the local file as a fallback.
    ///
    /// # Side Effects
    ///
    /// * Toggles `self.db_ready`.
    /// * Completes or rejects entries in `self.pending_logins`.
    /// * Deletes local save files on successful DB persistence.
    ///
    /// # Call Stack
    ///
    /// **Called by:** `Engine::cycle`
    /// **Calls:** `try_complete_login`, `delete_save_file`
    pub(crate) fn saves(&mut self) {
        while let Ok(msg) = self.db_rx.try_recv() {
            match msg {
                DbResponse::DbReady => {
                    self.db_ready = true;
                    info!("DB ready - logins enabled");
                }
                DbResponse::DbDisconnected => {
                    self.db_ready = false;
                    warn!("DB disconnected - logins disabled");
                    while !self.pending_logins.is_empty() {
                        let idx = self.pending_logins.len() - 1;
                        self.reject_pending_login(idx, LoginResponse::LoginServerOffline);
                    }
                }
                DbResponse::AuthResponse { user37, success } => {
                    if let Some(idx) = self.pending_logins.iter().position(|p| p.user37 == user37) {
                        if success {
                            self.pending_logins[idx].auth_ok = true;
                            self.try_complete_login(idx);
                        } else {
                            self.reject_pending_login(idx, LoginResponse::InvalidCredentials);
                        }
                    }
                }
                DbResponse::LoadResponse { user37, profile } => {
                    if let Some(idx) = self.pending_logins.iter().position(|p| p.user37 == user37) {
                        self.pending_logins[idx].profile = Some(profile);
                        self.try_complete_login(idx);
                    }
                }
                DbResponse::SaveAck {
                    user37: _,
                    username,
                    success,
                } => {
                    if success {
                        delete_save_file(&username);
                    } else {
                        warn!("DB save failed for '{}', keeping local file", username);
                    }
                }
            }
        }
    }
}
