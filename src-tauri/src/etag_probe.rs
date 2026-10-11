use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use reqwest::header::{ETAG, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, OrgConfig};
use crate::model::snapshot_is_stale;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EtagStoreFile {
    repos: HashMap<String, String>,
    org_repo_list_fetched_at: Option<String>,
    cached_repo_names: Vec<String>,
}

pub struct EtagStore {
    path: PathBuf,
    inner: Mutex<EtagStoreFile>,
}

impl EtagStore {
    pub fn load(path: PathBuf) -> Self {
        let inner = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default()
        } else {
            EtagStoreFile::default()
        };
        Self {
            path,
            inner: Mutex::new(inner),
        }
    }

    fn save(&self) -> Result<(), String> {
        let guard = self.inner.lock().map_err(|_| "etag lock")?;
        let json = serde_json::to_string_pretty(&*guard).map_err(|e| e.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn repo_etag(&self, repo_key: &str) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.repos.get(repo_key).cloned())
    }

    pub fn set_repo_etag(&self, repo_key: &str, etag: String) -> Result<(), String> {
        if let Ok(mut guard) = self.inner.lock() {
            guard.repos.insert(repo_key.to_string(), etag);
        }
        self.save()
    }

    pub fn cached_repo_names_if_fresh(&self) -> Option<Vec<String>> {
        let guard = self.inner.lock().ok()?;
        if guard.cached_repo_names.is_empty() {
            return None;
        }
        let fresh = guard.org_repo_list_fetched_at.as_ref().map_or(false, |t| {
            DateTime::parse_from_rfc3339(t)
                .map(|d| Utc::now() - d.with_timezone(&Utc) <= chrono::Duration::hours(1))
                .unwrap_or(false)
        });
        if fresh {
            Some(guard.cached_repo_names.clone())
        } else {
            None
        }
    }
}

pub struct ProbeDecision {
    pub skip_graphql: bool,
    pub banner: Option<String>,
    pub mark_stale: bool,
}

pub async fn evaluate_probe(
    client: &reqwest::Client,
    token: &str,
    config: &AppConfig,
    org: &OrgConfig,
    store: &EtagStore,
    last_graphql_fetched_at: &str,
) -> Result<ProbeDecision, String> {
    if !config.use_etag_probe {
        return Ok(ProbeDecision {
            skip_graphql: false,
            banner: None,
            mark_stale: false,
        });
    }

    let repos = repo_names_for_probe(client, token, org, store).await?;
    if repos.is_empty() {
        return Ok(ProbeDecision {
            skip_graphql: false,
            banner: None,
            mark_stale: false,
        });
    }

    let mut all_304 = true;
    for repo in repos {
        if !org_repo_allowed(&repo, org) {
            continue;
        }
        let key = format!("{}/{}", org.name, repo);
        let url = format!(
            "https://api.github.com/repos/{}/{}/issues?state=open&sort=updated&direction=desc&per_page=1",
            org.name,
            repo
        );
        let mut req = client
            .get(&url)
            .bearer_auth(token)
            .header("User-Agent", "agent-dashboard")
            .header("Accept", "application/vnd.github+json");
        if let Some(etag) = store.repo_etag(&key) {
            req = req.header(IF_NONE_MATCH, etag);
        }
        let response = req.send().await.map_err(|e| e.to_string())?;
        if response.status().as_u16() == 304 {
            continue;
        }
        all_304 = false;
        if let Some(etag) = response.headers().get(ETAG) {
            if let Ok(value) = etag.to_str() {
                store.set_repo_etag(&key, value.to_string())?;
            }
        }
    }

    let snapshot_fresh = !snapshot_is_stale(last_graphql_fetched_at, config.ci_max_staleness_seconds);
    if all_304 && snapshot_fresh {
        let mins = config.ci_max_staleness_seconds / 60;
        let banner = if mins > 0 {
            Some(format!(
                "CI may be up to {} minute(s) old (ETag probe skipped GraphQL).",
                mins.max(1)
            ))
        } else {
            Some("CI may be stale (ETag probe skipped GraphQL).".to_string())
        };
        return Ok(ProbeDecision {
            skip_graphql: true,
            banner,
            mark_stale: true,
        });
    }

    Ok(ProbeDecision {
        skip_graphql: false,
        banner: None,
        mark_stale: false,
    })
}

fn org_repo_allowed(repo: &str, org: &OrgConfig) -> bool {
    if !org.include.is_empty() && !org.include.contains(&repo.to_string()) {
        return false;
    }
    !org.exclude.contains(&repo.to_string())
}

async fn repo_names_for_probe(
    client: &reqwest::Client,
    token: &str,
    org: &OrgConfig,
    store: &EtagStore,
) -> Result<Vec<String>, String> {
    if !org.include.is_empty() {
        return Ok(org.include.clone());
    }

    if let Some(names) = store.cached_repo_names_if_fresh() {
        return Ok(names);
    }

    let url = format!(
        "https://api.github.com/orgs/{}/repos?per_page=100&type=all",
        org.name
    );
    let response = client
        .get(&url)
        .bearer_auth(token)
        .header("User-Agent", "agent-dashboard")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let text = response.text().await.map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let names = json
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if let Ok(mut guard) = store.inner.lock() {
        guard.cached_repo_names = names.clone();
        guard.org_repo_list_fetched_at = Some(Utc::now().to_rfc3339());
    }
    store.save()?;
    Ok(names)
}

pub fn etag_store_path(config_dir: &Path) -> PathBuf {
    config_dir.join("etag-store.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OrgConfig;

    #[test]
    fn include_exclude_filters() {
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            include: vec!["ham-radio-ui".to_string()],
            exclude: vec![],
        };
        assert!(org_repo_allowed("ham-radio-ui", &org));
        assert!(!org_repo_allowed("ham-radio-docs", &org));
    }

    #[test]
    fn probe_skips_when_all_304_and_fresh_snapshot() {
        let config = AppConfig {
            use_etag_probe: true,
            ci_max_staleness_seconds: 300,
            ..AppConfig::default()
        };
        let fetched = (Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        assert!(!snapshot_is_stale(&fetched, config.ci_max_staleness_seconds));
    }
}
