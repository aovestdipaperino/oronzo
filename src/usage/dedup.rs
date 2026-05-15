use crate::usage::parse::UsageRow;
use std::collections::HashSet;

pub fn dedup(rows: Vec<UsageRow>) -> Vec<UsageRow> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        if r.message_id.is_empty() || r.request_id.is_empty() {
            out.push(r);
            continue;
        }
        let key = (r.message_id.clone(), r.request_id.clone());
        if seen.insert(key) {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::parse;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn dedup_drops_duplicate_message_id() {
        let rows = parse::parse_all(&fixture("tests/fixtures/usage"));
        assert_eq!(rows.len(), 3, "fixtures contain 3 raw rows");
        let deduped = dedup(rows);
        assert_eq!(deduped.len(), 2, "duplicate (msg-001, req-001) collapsed");
    }

    #[test]
    fn dedup_keeps_rows_missing_ids() {
        let ts = chrono::DateTime::parse_from_rfc3339("2026-05-07T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let r = |mid: &str, rid: &str| UsageRow {
            timestamp: ts,
            model: "m".into(),
            project: "p".into(),
            session_id: "s".into(),
            message_id: mid.into(),
            request_id: rid.into(),
            input_tokens: 1,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        let rows = vec![r("", ""), r("", ""), r("a", "")];
        let deduped = dedup(rows);
        assert_eq!(deduped.len(), 3, "empty-id rows never collide");
    }
}
