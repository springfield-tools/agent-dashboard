use std::fs;
use std::path::Path;

use crate::config::write_file_atomic;
use crate::model::DashboardSnapshot;

pub fn load_snapshot_file(path: &Path) -> Option<DashboardSnapshot> {
    if !path.exists() {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_snapshot_file(path: &Path, snapshot: &DashboardSnapshot) -> Result<(), String> {
    let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
    write_file_atomic(path, &json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::model::build_dashboard;

    #[test]
    fn round_trip_snapshot_file() {
        let dir = std::env::temp_dir().join("agent-dashboard-snapshot-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("snapshot.json");
        let config = AppConfig::default();
        let snapshot = build_dashboard(vec![], &config, "2026-10-10T12:00:00Z");
        write_snapshot_file(&path, &snapshot).expect("write");
        let loaded = load_snapshot_file(&path).expect("load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.fetched_at, snapshot.fetched_at);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
