use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    dirty: AtomicBool,
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
            dirty: AtomicBool::new(false),
        }
    }

    fn save(&self) -> Result<(), String> {
        if !self.dirty.load(Ordering::SeqCst) {
            return Ok(());
        }
        let guard = self.inner.lock().map_err(|_| "etag lock")?;
        let json = serde_json::to_string_pretty(&*guard).map_err(|e| e.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.path, json).map_err(|e| e.to_string())?;
        self.dirty.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn repo_etag(&self, repo_key: &str) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.repos.get(repo_key).cloned())
    }

    pub fn set_repo_etag(&self, repo_key: &str, etag: String) -> Result<(), String> {
        let mut changed = false;
        if let Ok(mut guard) = self.inner.lock() {
            if guard.repos.get(repo_key) != Some(&etag) {
                guard.repos.insert(repo_key.to_string(), etag);
                changed = true;
            }
        }
        if changed {
            self.dirty.store(true, Ordering::SeqCst);
            self.save()?;
        }
        Ok(())
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

#[derive(Clone)]
pub struct ProbeDecision {
    pub skip_graphql: bool,
    pub banner: Option<String>,
}

/// Mirrors the org probe loop in `poll.rs`: any error or non-skip result means run GraphQL.
pub(crate) fn should_skip_graphql_after_org_probes(
    org_count: usize,
    outcomes: &[Result<ProbeDecision, String>],
) -> bool {
    if org_count == 0 {
        return false;
    }
    let mut skip_all = true;
    for outcome in outcomes {
        match outcome {
            Ok(p) if !p.skip_graphql => {
                skip_all = false;
                break;
            }
            Ok(_) => {}
            Err(_) => {
                skip_all = false;
                break;
            }
        }
    }
    skip_all
}

pub async fn evaluate_probe(
    client: &reqwest::Client,
    token: &str,
    config: &AppConfig,
    org: &OrgConfig,
    store: &EtagStore,
    last_graphql_fetched_at: &str,
    api_root: &str,
) -> Result<ProbeDecision, String> {
    if !config.use_etag_probe {
        return Ok(ProbeDecision {
            skip_graphql: false,
            banner: None,
        });
    }

    let repos = repo_names_for_probe(client, token, org, store, api_root).await?;
    if repos.is_empty() {
        return Ok(ProbeDecision {
            skip_graphql: false,
            banner: None,
        });
    }

    let mut all_304 = true;
    for repo in repos {
        if !org_repo_allowed(&repo, org) {
            continue;
        }
        let key = format!("{}/{}", org.name, repo);
        let url = format!(
            "{}/repos/{}/{}/issues?state=open&sort=updated&direction=desc&per_page=1",
            api_root.trim_end_matches('/'),
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
        });
    }

    Ok(ProbeDecision {
        skip_graphql: false,
        banner: None,
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
    api_root: &str,
) -> Result<Vec<String>, String> {
    if !org.include.is_empty() {
        return Ok(org.include.clone());
    }

    if let Some(names) = store.cached_repo_names_if_fresh() {
        return Ok(names);
    }

    let url = format!(
        "{}/orgs/{}/repos?per_page=100&type=all",
        api_root.trim_end_matches('/'),
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
    let names: Vec<String> = json
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if let Ok(mut guard) = store.inner.lock() {
        let changed = guard.cached_repo_names != names
            || guard.org_repo_list_fetched_at.is_none();
        guard.cached_repo_names = names.clone();
        guard.org_repo_list_fetched_at = Some(Utc::now().to_rfc3339());
        if changed {
            store.dirty.store(true, Ordering::SeqCst);
            store.save()?;
        }
    }
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
            credential: None,
            include: vec!["ham-radio-ui".to_string()],
            exclude: vec![],
        };
        assert!(org_repo_allowed("ham-radio-ui", &org));
        assert!(!org_repo_allowed("ham-radio-docs", &org));
    }

    #[test]
    fn probe_error_does_not_skip_graphql() {
        assert!(!should_skip_graphql_after_org_probes(
            1,
            &[Err("connection refused".to_string())],
        ));
    }

    #[test]
    fn probe_http_500_outcome_does_not_skip_graphql() {
        assert!(!should_skip_graphql_after_org_probes(
            1,
            &[Ok(ProbeDecision {
                skip_graphql: false,
                banner: None,
            })],
        ));
    }

    #[tokio::test]
    async fn probe_issues_500_falls_through_via_httpmock() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            credential: None,
            include: vec!["ham-radio-ui".to_string()],
            exclude: vec![],
        };
        let _m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/springfield-ham-radio/ham-radio-ui/issues");
            then.status(500);
        });

        let store = EtagStore::load(std::env::temp_dir().join("etag-test-500.json"));
        let config = AppConfig {
            use_etag_probe: true,
            ci_max_staleness_seconds: 300,
            ..AppConfig::default()
        };
        let fetched = (Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        let client = reqwest::Client::new();
        let decision = evaluate_probe(
            &client,
            "test-token",
            &config,
            &org,
            &store,
            &fetched,
            &server.base_url(),
        )
        .await
        .expect("probe");
        assert!(!decision.skip_graphql);
        assert!(!should_skip_graphql_after_org_probes(1, &[Ok(decision)]));
    }

    #[tokio::test]
    async fn all_304_skips_graphql_via_httpmock() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        let org = OrgConfig {
            name: "springfield-ham-radio".to_string(),
            credential: None,
            include: vec!["ham-radio-ui".to_string()],
            exclude: vec![],
        };
        let _m = server.mock(|when, then| {
            when.method(GET)
                .path("/repos/springfield-ham-radio/ham-radio-ui/issues");
            then.status(304);
        });

        let store = EtagStore::load(std::env::temp_dir().join("etag-test-store.json"));
        store
            .set_repo_etag("springfield-ham-radio/ham-radio-ui", "\"abc\"".to_string())
            .expect("etag");

        let config = AppConfig {
            use_etag_probe: true,
            ci_max_staleness_seconds: 300,
            ..AppConfig::default()
        };
        let fetched = (Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        let client = reqwest::Client::new();
        let decision = evaluate_probe(
            &client,
            "test-token",
            &config,
            &org,
            &store,
            &fetched,
            &server.base_url(),
        )
        .await
        .expect("probe");
        assert!(decision.skip_graphql);
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
