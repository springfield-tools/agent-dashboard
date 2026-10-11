use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::config::{AppConfig, OrgConfig};
use crate::keychain::{has_token_for_org, load_token_for_org, KeychainError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhTokenError {
    GhNotFound,
    NotLoggedIn,
    CommandFailed,
}

impl GhTokenError {
    pub fn banner_message(&self) -> String {
        match self {
            GhTokenError::GhNotFound => {
                "credential = \"gh\" but the GitHub CLI (gh) was not found. Install gh or set credential = \"keychain\" and paste a PAT in Settings.".to_string()
            }
            GhTokenError::NotLoggedIn => {
                "credential = \"gh\" but gh is not logged in. Run `gh auth login` in Terminal, or set credential = \"keychain\" and paste a PAT in Settings.".to_string()
            }
            GhTokenError::CommandFailed => {
                "credential = \"gh\" but `gh auth token` failed. Run `gh auth login` or switch to credential = \"keychain\".".to_string()
            }
        }
    }
}

pub fn uses_gh_credential(config: &AppConfig) -> bool {
    config.github.credential.trim().eq_ignore_ascii_case("gh")
}

pub fn org_uses_gh(org: &OrgConfig, config: &AppConfig) -> bool {
    org.credential_mode(&config.github.credential) == "gh"
}

pub fn credential_source_label(config: &AppConfig) -> &'static str {
    if uses_gh_credential(config) {
        "gh"
    } else {
        "keychain"
    }
}

pub fn org_credential_source_label(org: &OrgConfig, config: &AppConfig) -> &'static str {
    if org_uses_gh(org, config) {
        "gh"
    } else {
        "keychain"
    }
}

pub fn credential_is_ready(config: &AppConfig) -> bool {
    config
        .orgs
        .iter()
        .all(|org| org_credential_is_ready(org, config))
}

pub fn org_credential_is_ready(org: &OrgConfig, config: &AppConfig) -> bool {
    if org_uses_gh(org, config) {
        resolve_gh_executable().is_some()
    } else {
        has_token_for_org(&org.name)
    }
}

/// Budget bucket key shown in the footer (org name for keychain PATs, `gh` for CLI).
pub fn graphql_budget_key(org: &OrgConfig, config: &AppConfig) -> String {
    if org_uses_gh(org, config) {
        "gh".to_string()
    } else {
        org.name.clone()
    }
}

/// Search order: `PATH`, then Homebrew paths GUI apps often miss.
pub fn gh_executable_candidates() -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    let mut push = |p: PathBuf| {
        if seen.insert(p.clone()) {
            out.push(p);
        }
    };

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':').filter(|d| !d.is_empty()) {
            push(PathBuf::from(dir).join("gh"));
        }
    }
    push(PathBuf::from("/opt/homebrew/bin/gh"));
    push(PathBuf::from("/usr/local/bin/gh"));

    out
}

pub fn resolve_gh_executable() -> Option<PathBuf> {
    for candidate in gh_executable_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn read_token_from_gh_output(stdout: &[u8]) -> Option<String> {
    let token = String::from_utf8_lossy(stdout).trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

pub fn fetch_gh_token_from_binary(gh: &Path) -> Result<String, GhTokenError> {
    let output = Command::new(gh)
        .args(["auth", "token"])
        .output()
        .map_err(|_| GhTokenError::CommandFailed)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let lower = stderr.to_ascii_lowercase();
        if lower.contains("not logged") || lower.contains("no oauth") || lower.contains("auth login") {
            return Err(GhTokenError::NotLoggedIn);
        }
        return Err(GhTokenError::NotLoggedIn);
    }

    read_token_from_gh_output(&output.stdout).ok_or(GhTokenError::NotLoggedIn)
}

pub fn fetch_gh_token() -> Result<String, GhTokenError> {
    let gh = resolve_gh_executable().ok_or(GhTokenError::GhNotFound)?;
    fetch_gh_token_from_binary(&gh)
}

pub fn clear_gh_session_cache(cache: &Mutex<Option<String>>) {
    if let Ok(mut guard) = cache.lock() {
        *guard = None;
    }
}

pub fn obtain_github_token(
    config: &AppConfig,
    gh_cache: &Mutex<Option<String>>,
    invalidate_gh_cache: bool,
) -> Result<String, Option<String>> {
    let org = config
        .orgs
        .first()
        .ok_or(Some("No orgs configured.".to_string()))?;
    obtain_github_token_for_org(config, org, gh_cache, invalidate_gh_cache)
}

pub fn obtain_github_token_for_org(
    config: &AppConfig,
    org: &OrgConfig,
    gh_cache: &Mutex<Option<String>>,
    invalidate_gh_cache: bool,
) -> Result<String, Option<String>> {
    if org_uses_gh(org, config) {
        if invalidate_gh_cache {
            clear_gh_session_cache(gh_cache);
        } else if let Ok(guard) = gh_cache.lock() {
            if let Some(token) = guard.clone() {
                return Ok(token);
            }
        }

        match fetch_gh_token() {
            Ok(token) => {
                if let Ok(mut guard) = gh_cache.lock() {
                    *guard = Some(token.clone());
                }
                Ok(token)
            }
            Err(e) => Err(Some(e.banner_message())),
        }
    } else {
        match load_token_for_org(&org.name) {
            Ok(Some(token)) => Ok(token),
            Ok(None) => Err(Some(format!(
                "Add a GitHub token for org \"{}\" in Settings (credential = \"keychain\").",
                org.name
            ))),
            Err(KeychainError::Message(msg)) => Err(Some(msg)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn detects_gh_credential_mode() {
        let mut config = AppConfig::default();
        assert!(!uses_gh_credential(&config));
        config.github.credential = "gh".to_string();
        assert!(uses_gh_credential(&config));
        config.github.credential = "GH".to_string();
        assert!(uses_gh_credential(&config));
    }

    #[test]
    fn org_credential_override() {
        let mut config = AppConfig::default();
        config.github.credential = "keychain".to_string();
        let org = OrgConfig {
            name: "springfield-tools".to_string(),
            credential: Some("gh".to_string()),
            include: vec![],
            exclude: vec![],
        };
        assert!(org_uses_gh(&org, &config));
        assert_eq!(graphql_budget_key(&org, &config), "gh");
    }

    #[test]
    fn gh_candidates_include_homebrew_paths() {
        let paths = gh_executable_candidates();
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin/gh")));
        assert!(paths.contains(&PathBuf::from("/usr/local/bin/gh")));
    }

    #[test]
    fn parses_stdout_token() {
        assert_eq!(
            read_token_from_gh_output(b"  ghp_test\n"),
            Some("ghp_test".to_string())
        );
        assert_eq!(read_token_from_gh_output(b"\n"), None);
    }

    #[test]
    fn fake_gh_echoes_token() {
        let dir = std::env::temp_dir().join("agent-dashboard-fake-gh");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let gh = dir.join("gh");
        fs::write(&gh, "#!/bin/sh\necho ghp_from_fake_gh\n").expect("write");
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("chmod");

        let token = fetch_gh_token_from_binary(&gh).expect("token");
        assert_eq!(token, "ghp_from_fake_gh");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fake_gh_not_logged_in_exits_nonzero() {
        let dir = std::env::temp_dir().join("agent-dashboard-fake-gh-fail");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let gh = dir.join("gh");
        fs::write(
            &gh,
            "#!/bin/sh\necho 'not logged into any hosts' 1>&2\nexit 1\n",
        )
        .expect("write");
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("chmod");

        let err = fetch_gh_token_from_binary(&gh).unwrap_err();
        assert_eq!(err, GhTokenError::NotLoggedIn);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gh_cache_returns_cached_token_without_running_gh() {
        let mut config = AppConfig::default();
        config.github.credential = "gh".to_string();
        let cache = Mutex::new(Some("cached".to_string()));
        let org = config.orgs[0].clone();

        let token = obtain_github_token_for_org(&config, &org, &cache, false).expect("ok");
        assert_eq!(token, "cached");
    }

    #[test]
    fn clearing_gh_cache_drops_session_token() {
        let cache = Mutex::new(Some("secret".to_string()));
        clear_gh_session_cache(&cache);
        assert!(cache.lock().unwrap().is_none());
    }
}
