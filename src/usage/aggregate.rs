use crate::usage::args::UsageArgs;
use crate::usage::parse::UsageRow;
use crate::usage::pricing::Pricing;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

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

#[derive(Debug, Serialize, Clone, PartialEq)]
pub enum ReportKind { Daily, Weekly, Monthly, Session, Blocks }

#[derive(Debug, Serialize, Clone)]
pub struct Bucket {
    pub label: String,
    pub project: Option<String>,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub cost_usd: Option<f64>,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ReportData {
    pub kind: ReportKind,
    pub buckets: Vec<Bucket>,
}

#[derive(Default)]
struct Acc {
    project: Option<String>,
    model: Option<String>,
    input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
    per_model_cost: f64,
    cost_known: bool,
    first: Option<DateTime<Utc>>,
    last: Option<DateTime<Utc>>,
}

impl Acc {
    fn add(&mut self, r: &UsageRow, pricing: &Pricing) {
        self.input += r.input_tokens;
        self.output += r.output_tokens;
        self.cache_creation += r.cache_creation_tokens;
        self.cache_read += r.cache_read_tokens;
        if let Some(c) = pricing.compute_cost(
            &r.model,
            r.input_tokens,
            r.output_tokens,
            r.cache_creation_tokens,
            r.cache_read_tokens,
        ) {
            self.per_model_cost += c;
            self.cost_known = true;
        }
        self.first = Some(self.first.map_or(r.timestamp, |t| t.min(r.timestamp)));
        self.last = Some(self.last.map_or(r.timestamp, |t| t.max(r.timestamp)));
    }

    fn into_bucket(self, label: String) -> Bucket {
        Bucket {
            label,
            project: self.project,
            model: self.model,
            input: self.input,
            output: self.output,
            cache_creation: self.cache_creation,
            cache_read: self.cache_read,
            cost_usd: if self.cost_known { Some(self.per_model_cost) } else { None },
            first: self.first.unwrap_or_else(Utc::now),
            last: self.last.unwrap_or_else(Utc::now),
        }
    }
}

pub fn aggregate_daily(rows: Vec<UsageRow>, args: &UsageArgs, pricing: &Pricing) -> ReportData {
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for r in rows {
        let label = args.timezone.ymd_label(r.timestamp);
        acc.entry(label).or_default().add(&r, pricing);
    }
    ReportData {
        kind: ReportKind::Daily,
        buckets: acc.into_iter().map(|(label, a)| a.into_bucket(label)).collect(),
    }
}

pub fn aggregate_weekly(rows: Vec<UsageRow>, args: &UsageArgs, pricing: &Pricing) -> ReportData {
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for r in rows {
        let label = args.timezone.iso_week_label(r.timestamp);
        acc.entry(label).or_default().add(&r, pricing);
    }
    ReportData {
        kind: ReportKind::Weekly,
        buckets: acc.into_iter().map(|(l, a)| a.into_bucket(l)).collect(),
    }
}

pub fn aggregate_monthly(rows: Vec<UsageRow>, args: &UsageArgs, pricing: &Pricing) -> ReportData {
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for r in rows {
        let label = args.timezone.ym_label(r.timestamp);
        acc.entry(label).or_default().add(&r, pricing);
    }
    ReportData {
        kind: ReportKind::Monthly,
        buckets: acc.into_iter().map(|(l, a)| a.into_bucket(l)).collect(),
    }
}

pub fn aggregate_session(rows: Vec<UsageRow>, _args: &UsageArgs, pricing: &Pricing) -> ReportData {
    let mut acc: BTreeMap<(String, String), Acc> = BTreeMap::new();
    for r in rows {
        let key = (r.project.clone(), r.session_id.clone());
        let entry = acc.entry(key.clone()).or_default();
        if entry.project.is_none() {
            entry.project = Some(key.0.clone());
        }
        entry.add(&r, pricing);
    }
    let buckets = acc
        .into_iter()
        .map(|((project, session), a)| {
            let label = format!("{project} / {session}");
            let mut b = a.into_bucket(label);
            b.project = Some(project);
            b
        })
        .collect();
    ReportData { kind: ReportKind::Session, buckets }
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

    #[test]
    fn daily_buckets_rows_by_calendar_day() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let rows = vec![
            row("2026-05-07T10:00:00Z", "/p"),
            row("2026-05-07T12:00:00Z", "/p"),
            row("2026-05-08T01:00:00Z", "/p"),
        ];
        let report = aggregate_daily(rows, &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 2);
        assert_eq!(report.buckets[0].label, "2026-05-07");
        assert_eq!(report.buckets[0].input, 2);
        assert_eq!(report.buckets[1].label, "2026-05-08");
        assert_eq!(report.buckets[1].input, 1);
    }

    #[test]
    fn weekly_buckets_use_iso_week() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let rows = vec![
            row("2026-01-05T10:00:00Z", "/p"),  // ISO week 2026-W02
            row("2026-01-06T10:00:00Z", "/p"),  // same week
            row("2026-01-12T10:00:00Z", "/p"),  // 2026-W03
        ];
        let report = aggregate_weekly(rows, &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 2);
        assert_eq!(report.buckets[0].label, "2026-W02");
        assert_eq!(report.buckets[0].input, 2);
        assert_eq!(report.buckets[1].label, "2026-W03");
    }

    #[test]
    fn monthly_buckets_by_year_month() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let rows = vec![
            row("2026-04-30T10:00:00Z", "/p"),
            row("2026-05-01T10:00:00Z", "/p"),
            row("2026-05-30T10:00:00Z", "/p"),
        ];
        let report = aggregate_monthly(rows, &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 2);
        assert_eq!(report.buckets[0].label, "2026-04");
        assert_eq!(report.buckets[1].label, "2026-05");
        assert_eq!(report.buckets[1].input, 2);
    }

    #[test]
    fn session_buckets_by_project_session_pair() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let mut a = row("2026-05-07T10:00:00Z", "/p");
        a.session_id = "s1".into();
        let mut b = row("2026-05-07T11:00:00Z", "/p");
        b.session_id = "s1".into();
        let mut c = row("2026-05-07T12:00:00Z", "/p");
        c.session_id = "s2".into();
        let report = aggregate_session(vec![a, b, c], &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 2);
        // The s1 bucket aggregates two rows.
        let s1 = report.buckets.iter().find(|b| b.label.ends_with("s1")).unwrap();
        assert_eq!(s1.input, 2);
        assert_eq!(s1.first.to_rfc3339(), "2026-05-07T10:00:00+00:00");
        assert_eq!(s1.last.to_rfc3339(),  "2026-05-07T11:00:00+00:00");
    }
}
