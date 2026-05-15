use crate::usage::aggregate::ReportData;

pub fn to_json(report: &ReportData) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".into())
}

pub fn to_table(report: &ReportData) -> String {
    use comfy_table::{Cell, Table};
    let mut table = Table::new();
    table.set_header(vec![
        "Label", "Project", "Model", "Input", "Output", "Cache+", "CacheR", "Cost (USD)",
    ]);
    for b in &report.buckets {
        let cost = match b.cost_usd {
            Some(v) => format!("${v:.4}"),
            None => "—".into(),
        };
        table.add_row(vec![
            Cell::new(&b.label),
            Cell::new(b.project.as_deref().unwrap_or("—")),
            Cell::new(b.model.as_deref().unwrap_or("—")),
            Cell::new(b.input),
            Cell::new(b.output),
            Cell::new(b.cache_creation),
            Cell::new(b.cache_read),
            Cell::new(cost),
        ]);
    }
    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::aggregate::{Bucket, ReportKind};
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
    fn table_includes_label_and_token_columns() {
        let r = ReportData {
            kind: ReportKind::Daily,
            buckets: vec![bucket()],
        };
        let s = to_table(&r);
        assert!(s.contains("2026-05-07"));
        assert!(s.contains("100"));
        assert!(s.contains("50"));
        assert!(s.to_lowercase().contains("input"));
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
