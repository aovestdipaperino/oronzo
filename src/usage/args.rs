use chrono::NaiveDate;
use chrono_tz::Tz;

#[derive(Debug, PartialEq)]
pub enum Report {
    Daily,
    Weekly,
    Monthly,
    Session,
    Blocks,
}

#[derive(Debug)]
pub enum ActiveTz {
    Named(Tz),
    Local,
}

impl PartialEq for ActiveTz {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (ActiveTz::Local, ActiveTz::Local))
            || matches!((self, other),
                (ActiveTz::Named(a), ActiveTz::Named(b)) if a.name() == b.name())
    }
}

#[derive(Debug, PartialEq)]
pub struct UsageArgs {
    pub report: Report,
    pub since: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub project: Option<String>,
    pub breakdown: bool,
    pub instances: bool,
    pub timezone: ActiveTz,
    pub json: bool,
    pub offline: bool,
    pub debug: bool,
    pub active: bool,
    pub recent: usize,
}

impl Default for UsageArgs {
    fn default() -> Self {
        UsageArgs {
            report: Report::Daily,
            since: None,
            until: None,
            project: None,
            breakdown: false,
            instances: false,
            timezone: ActiveTz::Local,
            json: false,
            offline: false,
            debug: false,
            active: false,
            recent: 10,
        }
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .ok()
}

pub fn parse(args: &[String]) -> Result<UsageArgs, String> {
    let mut out = UsageArgs::default();
    let mut i = 0;
    // First positional (if any) is the report kind.
    if let Some(first) = args.first() {
        match first.as_str() {
            "daily" => { out.report = Report::Daily; i = 1; }
            "weekly" => { out.report = Report::Weekly; i = 1; }
            "monthly" => { out.report = Report::Monthly; i = 1; }
            "session" => { out.report = Report::Session; i = 1; }
            "blocks" => { out.report = Report::Blocks; i = 1; }
            s if !s.starts_with('-') => {
                return Err(format!("unknown report: {s}"));
            }
            _ => {}
        }
    }
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--since" => {
                let v = args.get(i + 1).ok_or("--since requires a date")?;
                out.since = parse_date(v).ok_or_else(|| format!("bad --since: {v}"))?.into();
                i += 2;
            }
            "--until" => {
                let v = args.get(i + 1).ok_or("--until requires a date")?;
                out.until = parse_date(v).ok_or_else(|| format!("bad --until: {v}"))?.into();
                i += 2;
            }
            "--project" => {
                let v = args.get(i + 1).ok_or("--project requires a value")?;
                out.project = Some(v.clone());
                i += 2;
            }
            "--breakdown" => { out.breakdown = true; i += 1; }
            "--instances" => { out.instances = true; i += 1; }
            "--timezone" => {
                let v = args.get(i + 1).ok_or("--timezone requires a value")?;
                let tz: Tz = v.parse().map_err(|_| format!("unknown timezone: {v}"))?;
                out.timezone = ActiveTz::Named(tz);
                i += 2;
            }
            "--json" => { out.json = true; i += 1; }
            "--offline" => { out.offline = true; i += 1; }
            "--debug" => { out.debug = true; i += 1; }
            "--active" => { out.active = true; i += 1; }
            "--recent" => {
                let v = args.get(i + 1).ok_or("--recent requires a count")?;
                out.recent = v.parse().map_err(|_| format!("bad --recent: {v}"))?;
                i += 2;
            }
            _ => return Err(format!("unknown flag: {a}")),
        }
    }
    if out.instances && matches!(out.report, Report::Session | Report::Blocks) {
        return Err("--instances is only valid for daily/weekly/monthly".into());
    }
    if out.breakdown && matches!(out.report, Report::Session | Report::Blocks) {
        return Err("--breakdown is only valid for daily/weekly/monthly".into());
    }
    if out.active && out.report != Report::Blocks {
        return Err("--active is only valid for the blocks report".into());
    }
    if out.recent != 10 && out.report != Report::Blocks {
        return Err("--recent is only valid for the blocks report".into());
    }
    Ok(out)
}

use chrono::{DateTime, Datelike, Local, TimeZone, Utc};

impl ActiveTz {
    pub fn date_of(&self, ts: DateTime<Utc>) -> chrono::NaiveDate {
        match self {
            ActiveTz::Named(tz) => ts.with_timezone(tz).date_naive(),
            ActiveTz::Local => ts.with_timezone(&Local).date_naive(),
        }
    }

    pub fn ymd_label(&self, ts: DateTime<Utc>) -> String {
        self.date_of(ts).format("%Y-%m-%d").to_string()
    }

    pub fn ym_label(&self, ts: DateTime<Utc>) -> String {
        self.date_of(ts).format("%Y-%m").to_string()
    }

    pub fn iso_week_label(&self, ts: DateTime<Utc>) -> String {
        let d = self.date_of(ts);
        let iso = d.iso_week();
        format!("{}-W{:02}", iso.year(), iso.week())
    }

    pub fn start_of_day_utc(&self, d: chrono::NaiveDate) -> DateTime<Utc> {
        let naive = d.and_hms_opt(0, 0, 0).unwrap();
        match self {
            ActiveTz::Named(tz) => tz.from_local_datetime(&naive).unwrap().with_timezone(&Utc),
            ActiveTz::Local => Local.from_local_datetime(&naive).unwrap().with_timezone(&Utc),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_args_defaults_to_daily() {
        let a = parse(&argv(&[])).unwrap();
        assert_eq!(a.report, Report::Daily);
    }

    #[test]
    fn each_report_subcommand_maps() {
        for (name, kind) in [
            ("daily", Report::Daily),
            ("weekly", Report::Weekly),
            ("monthly", Report::Monthly),
            ("session", Report::Session),
            ("blocks", Report::Blocks),
        ] {
            let a = parse(&argv(&[name])).unwrap();
            assert_eq!(a.report, kind);
        }
    }

    #[test]
    fn since_until_parse_both_formats() {
        let a = parse(&argv(&["daily", "--since", "20260101", "--until", "2026-02-01"])).unwrap();
        assert_eq!(a.since, NaiveDate::from_ymd_opt(2026, 1, 1));
        assert_eq!(a.until, NaiveDate::from_ymd_opt(2026, 2, 1));
    }

    #[test]
    fn bad_date_errors() {
        assert!(parse(&argv(&["daily", "--since", "not-a-date"])).is_err());
    }

    #[test]
    fn parses_string_and_bool_flags() {
        let a = parse(&argv(&[
            "monthly",
            "--project", "frontend",
            "--breakdown",
            "--instances",
            "--json",
            "--offline",
            "--debug",
        ])).unwrap();
        assert_eq!(a.report, Report::Monthly);
        assert_eq!(a.project.as_deref(), Some("frontend"));
        assert!(a.breakdown);
        assert!(a.instances);
        assert!(a.json);
        assert!(a.offline);
        assert!(a.debug);
    }

    #[test]
    fn parses_blocks_specific_flags() {
        let a = parse(&argv(&["blocks", "--active", "--recent", "5"])).unwrap();
        assert!(a.active);
        assert_eq!(a.recent, 5);
    }

    #[test]
    fn parses_timezone() {
        let a = parse(&argv(&["daily", "--timezone", "America/Los_Angeles"])).unwrap();
        assert_eq!(a.timezone, ActiveTz::Named(chrono_tz::America::Los_Angeles));
    }

    #[test]
    fn rejects_instances_with_session_or_blocks() {
        assert!(parse(&argv(&["session", "--instances"])).is_err());
        assert!(parse(&argv(&["blocks", "--instances"])).is_err());
    }

    #[test]
    fn rejects_breakdown_with_session_or_blocks() {
        assert!(parse(&argv(&["session", "--breakdown"])).is_err());
        assert!(parse(&argv(&["blocks", "--breakdown"])).is_err());
    }

    #[test]
    fn rejects_active_without_blocks() {
        assert!(parse(&argv(&["daily", "--active"])).is_err());
    }

    #[test]
    fn date_of_handles_timezone_shift_across_midnight() {
        let ts = chrono::DateTime::parse_from_rfc3339("2026-05-07T23:30:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let tz = ActiveTz::Named(chrono_tz::Asia::Tokyo); // UTC+9
        assert_eq!(
            tz.date_of(ts),
            chrono::NaiveDate::from_ymd_opt(2026, 5, 8).unwrap()
        );
        let utc = ActiveTz::Named(chrono_tz::UTC);
        assert_eq!(
            utc.date_of(ts),
            chrono::NaiveDate::from_ymd_opt(2026, 5, 7).unwrap()
        );
    }
}
