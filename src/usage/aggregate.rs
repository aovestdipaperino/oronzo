use crate::usage::args::UsageArgs;
use crate::usage::parse::UsageRow;

pub fn filter(rows: Vec<UsageRow>, args: &UsageArgs) -> Vec<UsageRow> {
    let tz = &args.timezone;
    let since_lo = args.since.map(|d| tz.start_of_day_utc(d));
    let until_hi = args
        .until
        .map(|d| tz.start_of_day_utc(d.succ_opt().unwrap())); // exclusive upper

    rows.into_iter()
        .filter(|r| {
            if let Some(lo) = since_lo {
                if r.timestamp < lo {
                    return false;
                }
            }
            if let Some(hi) = until_hi {
                if r.timestamp >= hi {
                    return false;
                }
            }
            if let Some(p) = &args.project {
                if !r.project.contains(p) {
                    return false;
                }
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::args::ActiveTz;
    use chrono::{DateTime, NaiveDate, Utc};

    fn row(ts: &str, project: &str) -> UsageRow {
        UsageRow {
            timestamp: DateTime::parse_from_rfc3339(ts)
                .unwrap()
                .with_timezone(&Utc),
            model: "m".into(),
            project: project.into(),
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
    fn since_until_are_inclusive_in_timezone() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        args.since = NaiveDate::from_ymd_opt(2026, 5, 7);
        args.until = NaiveDate::from_ymd_opt(2026, 5, 7);
        let rows = vec![
            row("2026-05-06T23:00:00Z", "/p"),
            row("2026-05-07T00:00:00Z", "/p"),
            row("2026-05-07T23:59:00Z", "/p"),
            row("2026-05-08T00:00:00Z", "/p"),
        ];
        let filtered = filter(rows, &args);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn project_substring_filter() {
        let mut args = UsageArgs::default();
        args.project = Some("front".into());
        let rows = vec![row("2026-05-07T10:00:00Z", "/code/frontend"),
                        row("2026-05-07T10:00:00Z", "/code/backend")];
        let filtered = filter(rows, &args);
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].project.contains("front"));
    }
}
