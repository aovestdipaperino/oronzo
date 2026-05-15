use crate::usage::aggregate::{Bucket, ReportData};

pub fn to_json(report: &ReportData) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".into())
}

pub fn to_table(_report: &ReportData) -> String {
    String::new() // implemented in task 21
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::aggregate::ReportKind;
    use chrono::Utc;

    fn bucket() -> Bucket {
        Bucket {
            label: "2026-05-07".into(),
            project: None,
            model: None,
            input: 100,
            output: 50,
            cache_creation: 0,
            cache_read: 0,
            cost_usd: Some(0.001),
            first: Utc::now(),
            last: Utc::now(),
        }
    }

    #[test]
    fn json_contains_label_and_tokens() {
        let r = ReportData {
            kind: ReportKind::Daily,
            buckets: vec![bucket()],
        };
        let s = to_json(&r);
        assert!(s.contains("\"label\": \"2026-05-07\""));
        assert!(s.contains("\"input\": 100"));
        assert!(s.contains("\"cost_usd\": 0.001"));
    }
}
