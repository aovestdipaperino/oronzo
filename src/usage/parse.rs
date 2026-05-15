use chrono::{DateTime, Utc};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub timestamp: DateTime<Utc>,
    pub model: String,
    pub project: String,
    pub session_id: String,
    pub message_id: String,
    pub request_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

pub fn extract_rows(path: &Path) -> Vec<UsageRow> {
    let mut rows = Vec::new();
    let Ok(file) = fs::File::open(path) else {
        return rows;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if obj.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let Some(usage) = obj.pointer("/message/usage") else {
            continue;
        };
        let Some(ts_str) = obj.get("timestamp").and_then(|t| t.as_str()) else {
            continue;
        };
        let Ok(timestamp) = DateTime::parse_from_rfc3339(ts_str) else {
            continue;
        };
        let get_u64 = |k: &str| usage.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        rows.push(UsageRow {
            timestamp: timestamp.with_timezone(&Utc),
            model: obj
                .pointer("/message/model")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string(),
            project: obj
                .get("cwd")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            session_id: obj
                .get("sessionId")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            message_id: obj
                .pointer("/message/id")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string(),
            request_id: obj
                .get("requestId")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string(),
            input_tokens: get_u64("input_tokens"),
            output_tokens: get_u64("output_tokens"),
            cache_creation_tokens: get_u64("cache_creation_input_tokens"),
            cache_read_tokens: get_u64("cache_read_input_tokens"),
        });
    }
    rows
}

use crate::sessions;

pub fn parse_all(claude_dir: &Path) -> Vec<UsageRow> {
    let files = sessions::discover(claude_dir);
    let mut rows = Vec::new();
    for sf in files {
        rows.extend(extract_rows(&sf.path));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn extracts_assistant_usage_row() {
        let rows = extract_rows(&fixture("tests/fixtures/usage/proj_a/session_a1.jsonl"));
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.model, "claude-sonnet-4-6");
        assert_eq!(r.project, "/tmp/proj_a");
        assert_eq!(r.session_id, "sess-a");
        assert_eq!(r.message_id, "msg-001");
        assert_eq!(r.request_id, "req-001");
        assert_eq!(r.input_tokens, 100);
        assert_eq!(r.output_tokens, 50);
        assert_eq!(r.cache_creation_tokens, 10);
        assert_eq!(r.cache_read_tokens, 5);
    }

    #[test]
    fn skips_user_and_missing_usage() {
        let rows = extract_rows(&fixture("tests/fixtures/usage/proj_a/session_a1.jsonl"));
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn parse_all_walks_every_project() {
        let dir = fixture("tests/fixtures/usage");
        let rows = parse_all(&dir);
        assert_eq!(rows.len(), 3);
        let projects: std::collections::HashSet<&str> =
            rows.iter().map(|r| r.project.as_str()).collect();
        assert!(projects.contains("/tmp/proj_a"));
        assert!(projects.contains("/tmp/proj_b"));
    }
}
