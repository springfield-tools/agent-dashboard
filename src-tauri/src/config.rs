use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const POLL_INTERVAL_MIN: u64 = 15;
pub const POLL_INTERVAL_MAX: u64 = 900;
pub const DEFAULT_POLL_INTERVAL: u64 = 30;

const KNOWN_ROOT_KEYS: [&str; 11] = [
    "poll_interval_seconds",
    "pause_when_idle",
    "idle_pause_after_seconds",
    "use_etag_probe",
    "ci_max_staleness_seconds",
    "poll_pause",
    "github",
    "filters",
    "tray",
    "labels",
    "orgs",
];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Io(String),
    #[error("invalid TOML: {0}")]
    Parse(String),
    #[error("poll_interval_seconds must be between {min} and {max}, got {value}")]
    PollInterval { value: u64, min: u64, max: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BotFilterMode {
    Hide,
    Collapse,
    Show,
}

impl Default for BotFilterMode {
    fn default() -> Self {
        Self::Collapse
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiltersConfig {
    #[serde(default)]
    pub bots: BotFilterMode,
    #[serde(default = "default_bot_logins")]
    pub bot_logins: Vec<String>,
    #[serde(default = "default_true")]
    pub surface_failing_bots: bool,
}

fn default_bot_logins() -> Vec<String> {
    vec!["dependabot[bot]".to_string(), "renovate[bot]".to_string()]
}

fn default_true() -> bool {
    true
}

impl Default for FiltersConfig {
    fn default() -> Self {
        Self {
            bots: BotFilterMode::Collapse,
            bot_logins: default_bot_logins(),
            surface_failing_bots: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelsConfig {
    #[serde(default = "default_agent_working")]
    pub agent_working: String,
    #[serde(default = "default_needs_testing")]
    pub needs_testing: String,
    #[serde(default = "default_needs_input")]
    pub needs_input: String,
    #[serde(default = "default_ready_to_merge")]
    pub ready_to_merge: String,
}

fn default_agent_working() -> String {
    "agent-working".to_string()
}
fn default_needs_testing() -> String {
    "needs-testing".to_string()
}
fn default_needs_input() -> String {
    "needs-input".to_string()
}
fn default_ready_to_merge() -> String {
    "ready-to-merge".to_string()
}

impl Default for LabelsConfig {
    fn default() -> Self {
        Self {
            agent_working: default_agent_working(),
            needs_testing: default_needs_testing(),
            needs_input: default_needs_input(),
            ready_to_merge: default_ready_to_merge(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgConfig {
    pub name: String,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubConfig {
    #[serde(default = "default_credential")]
    pub credential: String,
}

fn default_credential() -> String {
    "keychain".to_string()
}

impl Default for GithubConfig {
    fn default() -> Self {
        Self {
            credential: default_credential(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrayConfig {
    #[serde(default = "default_tray_counts")]
    pub counts: Vec<String>,
}

fn default_tray_counts() -> Vec<String> {
    vec![
        "needs-input".to_string(),
        "needs-testing".to_string(),
        "ci-failing".to_string(),
        "ready-to-merge".to_string(),
    ]
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            counts: default_tray_counts(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    /// `"manual"` or an RFC3339 timestamp when polling resumes.
    #[serde(default)]
    pub poll_pause: Option<String>,
    #[serde(default)]
    pub pause_when_idle: bool,
    #[serde(default = "default_idle_pause")]
    pub idle_pause_after_seconds: u64,
    #[serde(default)]
    pub use_etag_probe: bool,
    #[serde(default = "default_ci_staleness")]
    pub ci_max_staleness_seconds: u64,
    #[serde(default)]
    pub github: GithubConfig,
    #[serde(default)]
    pub filters: FiltersConfig,
    #[serde(default)]
    pub tray: TrayConfig,
    #[serde(default)]
    pub labels: LabelsConfig,
    #[serde(default = "default_orgs")]
    pub orgs: Vec<OrgConfig>,
}

fn default_poll_interval() -> u64 {
    DEFAULT_POLL_INTERVAL
}

fn default_idle_pause() -> u64 {
    300
}

fn default_ci_staleness() -> u64 {
    30
}

fn default_orgs() -> Vec<OrgConfig> {
    vec![OrgConfig {
        name: "springfield-ham-radio".to_string(),
        include: vec![],
        exclude: vec![],
    }]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: DEFAULT_POLL_INTERVAL,
            poll_pause: None,
            pause_when_idle: false,
            idle_pause_after_seconds: 300,
            use_etag_probe: false,
            ci_max_staleness_seconds: 30,
            github: GithubConfig::default(),
            filters: FiltersConfig::default(),
            tray: TrayConfig::default(),
            labels: LabelsConfig::default(),
            orgs: default_orgs(),
        }
    }
}

pub fn validate_poll_interval(seconds: u64) -> Result<(), ConfigError> {
    if seconds < POLL_INTERVAL_MIN || seconds > POLL_INTERVAL_MAX {
        Err(ConfigError::PollInterval {
            value: seconds,
            min: POLL_INTERVAL_MIN,
            max: POLL_INTERVAL_MAX,
        })
    } else {
        Ok(())
    }
}

fn log_unknown_root_keys(text: &str) {
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(_) => return,
    };
    let known: HashSet<&str> = KNOWN_ROOT_KEYS.iter().copied().collect();
    for key in table.keys() {
        if !known.contains(key.as_str()) {
            log::warn!("config: unknown root key ignored: {key}");
        }
    }
}

pub fn parse_config_toml(text: &str) -> Result<AppConfig, ConfigError> {
    log_unknown_root_keys(text);
    let config: AppConfig = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
    validate_poll_interval(config.poll_interval_seconds)?;
    Ok(config)
}

/// True when the file is missing or has no non-whitespace content (including 0-byte files).
pub fn is_config_missing_or_blank(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }
    match fs::metadata(path) {
        Ok(meta) if meta.len() == 0 => return true,
        Ok(_) => {}
        Err(_) => return false,
    }
    match fs::read_to_string(path) {
        Ok(text) => text.trim().is_empty(),
        Err(_) => false,
    }
}

pub fn write_file_atomic(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config.toml".to_string());
    let temp_path = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temp_path, content).map_err(|e| format!("write temp file: {e}"))?;
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        format!("rename into place: {e}")
    })?;
    Ok(())
}

/// Write the commented example to `path` via a temp file in the same directory, then rename.
pub fn write_example_config_atomic(path: &Path) -> Result<(), String> {
    let content = example_config_text();
    if content.trim().is_empty() {
        return Err("embedded example config is empty".to_string());
    }
    write_file_atomic(path, content)
}

fn seed_default_config_file(path: &Path) -> (AppConfig, Option<String>) {
    let defaults = AppConfig::default();
    match write_example_config_atomic(path) {
        Ok(()) => {
            log::info!("config: wrote default config to {}", path.display());
            (defaults, None)
        }
        Err(e) => {
            log::warn!("config: could not write default config to {}: {e}", path.display());
            (defaults, Some(e))
        }
    }
}

pub fn load_config(path: &Path) -> (AppConfig, Option<String>) {
    if is_config_missing_or_blank(path) {
        return seed_default_config_file(path);
    }

    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            let msg = format!("failed to read config: {e}");
            log::warn!("config: {msg}");
            return (AppConfig::default(), Some(msg));
        }
    };

    match parse_config_toml(&text) {
        Ok(config) => (config, None),
        Err(e) => {
            log::warn!("config: parse failed, using defaults: {e}");
            (AppConfig::default(), Some(e.to_string()))
        }
    }
}

pub fn example_config_text() -> &'static str {
    include_str!("../../config.example.toml")
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    validate_poll_interval(config.poll_interval_seconds)?;
    let text = toml::to_string_pretty(config).map_err(|e| e.to_string())?;
    write_file_atomic(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_poll_interval_in_range() {
        let text = "poll_interval_seconds = 60\n";
        let config = parse_config_toml(text).expect("parse");
        assert_eq!(config.poll_interval_seconds, 60);
    }

    #[test]
    fn rejects_poll_interval_below_min() {
        let text = "poll_interval_seconds = 14\n";
        let err = parse_config_toml(text).unwrap_err();
        assert!(matches!(err, ConfigError::PollInterval { value: 14, .. }));
    }

    #[test]
    fn rejects_poll_interval_above_max() {
        let text = "poll_interval_seconds = 901\n";
        let err = parse_config_toml(text).unwrap_err();
        assert!(matches!(err, ConfigError::PollInterval { value: 901, .. }));
    }

    #[test]
    fn defaults_bots_to_collapse() {
        let config = parse_config_toml("poll_interval_seconds = 30\n").expect("parse");
        assert_eq!(config.filters.bots, BotFilterMode::Collapse);
    }

    #[test]
    fn first_launch_template_is_example_toml() {
        assert!(example_config_text().contains("[filters]"));
        assert!(example_config_text().contains("bots = \"collapse\""));
        assert!(example_config_text().contains("# hide | collapse | show"));
        assert!(!example_config_text().trim().is_empty());
    }

    #[test]
    fn empty_existing_file_is_replaced_with_example() {
        let dir = std::env::temp_dir().join("agent-dashboard-config-empty-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("config.toml");
        fs::write(&path, "").expect("empty file");

        let (config, err) = load_config(&path);
        assert!(err.is_none());
        assert_eq!(config.poll_interval_seconds, DEFAULT_POLL_INTERVAL);

        let written = fs::read_to_string(&path).expect("read config");
        assert!(written.contains("[filters]"));
        assert!(written.contains("bots = \"collapse\""));
        assert!(!written.trim().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trips_poll_pause() {
        let dir = std::env::temp_dir().join("agent-dashboard-config-pause-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("config.toml");
        let mut config = AppConfig::default();
        config.poll_pause = Some("manual".to_string());
        save_config(&path, &config).expect("save");
        let (loaded, err) = load_config(&path);
        assert!(err.is_none());
        assert_eq!(loaded.poll_pause.as_deref(), Some("manual"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_nonempty_config_is_not_overwritten() {
        let dir = std::env::temp_dir().join("agent-dashboard-config-invalid-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("config.toml");
        let invalid = "poll_interval_seconds = 14\n";
        fs::write(&path, invalid).expect("write invalid");

        let (_config, err) = load_config(&path);
        assert!(err.is_some());

        let on_disk = fs::read_to_string(&path).expect("read config");
        assert_eq!(on_disk, invalid);

        let _ = fs::remove_dir_all(&dir);
    }
}
