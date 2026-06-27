pub mod network;

/// Login response codes
#[repr(u8)]
pub enum LoginResponse {
    SuccessNormal = 2,
    InvalidCredentials = 3,
    AccountDisabled = 4,
    AlreadyLoggedIn = 5,
    RuneScapeUpdated = 6,
    WorldFull = 7,
    LoginServerOffline = 8,
    TooManyConnections = 9,
    BadSession = 10,
    Rejected = 11,
    MembersOnly = 12,
    CouldNotComplete = 13,
    ServerUpdating = 14,
    Reconnect = 15,
    TooManyAttempts = 16,
    MembersArea = 17,
    SuccessModerator = 18,
    #[cfg(since_244)]
    SuccessJagexModerator = 19,
}
