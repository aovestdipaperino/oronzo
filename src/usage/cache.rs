use crate::usage::parse::UsageRow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub mtime: f64,
    pub rows: Vec<UsageRow>,
}

#[derive(Default)]
pub struct Cache {
    pub path: PathBuf,
    pub entries: HashMap<String, Entry>,
}

impl Cache {
    pub fn default_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oronzo")
            .join("usage.json")
    }

    pub fn load(path: PathBuf) -> Self {
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Cache { path, entries }
    }

    pub fn get(&self, key: &str, mtime: f64) -> Option<&Entry> {
        self.entries
            .get(key)
            .filter(|e| (e.mtime - mtime).abs() < 1e-6)
    }

    pub fn set(&mut self, key: String, entry: Entry) {
        self.entries.insert(key, entry);
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&self.entries) {
            let _ = fs::write(&self.path, json);
        }
    }
}

pub fn file_mtime(p: &Path) -> f64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    fn row() -> UsageRow {
        UsageRow {
            timestamp: Utc::now(),
            model: "m".into(),
            project: "p".into(),
            session_id: "s".into(),
            message_id: "mid".into(),
            request_id: "rid".into(),
            input_tokens: 1,
            output_tokens: 1,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    #[test]
    fn round_trips_through_disk() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        let mut c = Cache::load(path.clone());
        c.set(
            "/file/a".into(),
            Entry {
                mtime: 1.0,
                rows: vec![row()],
            },
        );
        c.save();
        let loaded = Cache::load(path);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries["/file/a"].rows.len(), 1);
    }

    #[test]
    fn get_returns_none_on_mtime_mismatch() {
        let mut c = Cache::default();
        c.set(
            "/file/a".into(),
            Entry {
                mtime: 1.0,
                rows: vec![],
            },
        );
        assert!(c.get("/file/a", 1.0).is_some());
        assert!(c.get("/file/a", 2.0).is_none());
    }
}
