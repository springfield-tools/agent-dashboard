const SERVICE: &str = "com.springfield.agent-dashboard";
const ACCOUNT: &str = "github-token";

#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("keychain error: {0}")]
    Message(String),
}

pub fn store_token(token: &str) -> Result<(), KeychainError> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(KeychainError::Message("token is empty".to_string()));
    }
    #[cfg(target_os = "macos")]
    {
        use keyring::Entry;
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|e| KeychainError::Message(e.to_string()))?;
        entry
            .set_password(trimmed)
            .map_err(|e| KeychainError::Message(e.to_string()))?;
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = trimmed;
        Err(KeychainError::Message("keychain is only supported on macOS".to_string()))
    }
}

pub fn load_token() -> Result<Option<String>, KeychainError> {
    #[cfg(target_os = "macos")]
    {
        use keyring::Entry;
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|e| KeychainError::Message(e.to_string()))?;
        match entry.get_password() {
            Ok(value) if value.trim().is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeychainError::Message(e.to_string())),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(None)
    }
}

pub fn delete_token() -> Result<(), KeychainError> {
    #[cfg(target_os = "macos")]
    {
        use keyring::Entry;
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|e| KeychainError::Message(e.to_string()))?;
        match entry.delete_password() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeychainError::Message(e.to_string())),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

pub fn has_token() -> bool {
    load_token()
        .ok()
        .flatten()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
}
