use crate::ProtocolError;

/// Login type
#[repr(u8)]
pub enum LoginType {
    /// New game world login
    New = 16,
    /// Reconnect (after disconnect)
    Reconnect = 18,
}

impl TryFrom<u8> for LoginType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            16 => Ok(LoginType::New),
            18 => Ok(LoginType::Reconnect),
            _ => Err(ProtocolError::Login(
                format!("Invalid login type: {value}",),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_login_type_from_u8() {
        assert!(matches!(LoginType::try_from(16), Ok(LoginType::New)));
        assert!(matches!(LoginType::try_from(18), Ok(LoginType::Reconnect)));
        assert!(LoginType::try_from(99).is_err());
    }
}
