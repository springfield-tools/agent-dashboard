const SERVICE: &str = "com.springfield.agent-dashboard";
const LEGACY_ACCOUNT: &str = "github-token";

#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("keychain error: {0}")]
    Message(String),
}

pub fn account_for_org(org: &str) -> String {
    format!("github-token-{org}")
}

fn load_token_for_account(account: &str) -> Result<Option<String>, KeychainError> {
    #[cfg(target_os = "macos")]
    {
        use keyring::Entry;
        let entry = Entry::new(SERVICE, account).map_err(|e| KeychainError::Message(e.to_string()))?;
        match entry.get_password() {
            Ok(value) if value.trim().is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeychainError::Message(e.to_string())),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = account;
        Ok(None)
    }
}

fn store_token_for_account(account: &str, token: &str) -> Result<(), KeychainError> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(KeychainError::Message("token is empty".to_string()));
    }
    #[cfg(target_os = "macos")]
    {
        use keyring::Entry;
        let entry = Entry::new(SERVICE, account).map_err(|e| KeychainError::Message(e.to_string()))?;
        entry
            .set_password(trimmed)
            .map_err(|e| KeychainError::Message(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (account, trimmed);
        Err(KeychainError::Message("keychain is only supported on macOS".to_string()))
    }
}

fn delete_token_for_account(account: &str) -> Result<(), KeychainError> {
    #[cfg(target_os = "macos")]
    {
        use keyring::Entry;
        let entry = Entry::new(SERVICE, account).map_err(|e| KeychainError::Message(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeychainError::Message(e.to_string())),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = account;
        Ok(())
    }
}

/// Copies the legacy single-item PAT into the per-org account when needed.
pub fn migrate_legacy_token_to_org(org: &str) -> Result<(), KeychainError> {
    let account = account_for_org(org);
    if has_token_for_account(&account)? {
        return Ok(());
    }
    let legacy = load_token_for_account(LEGACY_ACCOUNT)?;
    if let Some(token) = legacy {
        store_token_for_account(&account, &token)?;
        match load_token_for_account(&account)? {
            Some(saved) if saved == token => {
                delete_token_for_account(LEGACY_ACCOUNT)?;
            }
            _ => {
                return Err(KeychainError::Message(
                    "copied the legacy GitHub token but could not read it back; left the original Keychain item in place"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}

pub fn store_token_for_org(org: &str, token: &str) -> Result<(), KeychainError> {
    store_token_for_account(&account_for_org(org), token)
}

pub fn load_token_for_org(org: &str) -> Result<Option<String>, KeychainError> {
    let account = account_for_org(org);
    let direct = load_token_for_account(&account)?;
    if direct.is_some() {
        return Ok(direct);
    }
    if org == "springfield-ham-radio" {
        let legacy = load_token_for_account(LEGACY_ACCOUNT)?;
        if legacy.is_some() {
            migrate_legacy_token_to_org(org)?;
            return load_token_for_account(&account);
        }
    }
    Ok(None)
}

pub fn delete_token_for_org(org: &str) -> Result<(), KeychainError> {
    delete_token_for_account(&account_for_org(org))?;
    if org == "springfield-ham-radio" {
        let _ = delete_token_for_account(LEGACY_ACCOUNT);
    }
    Ok(())
}

pub fn has_token_for_org(org: &str) -> bool {
    load_token_for_org(org)
        .ok()
        .flatten()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
}

fn has_token_for_account(account: &str) -> Result<bool, KeychainError> {
    Ok(load_token_for_account(account)?
        .map(|t| !t.is_empty())
        .unwrap_or(false))
}
