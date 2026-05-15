use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/usage")
}

#[test]
fn usage_daily_json_against_fixtures() {
    use oronzo::usage::{aggregate, args, dedup, parse, pricing, render};
    let mut a = args::UsageArgs::default();
    a.timezone = args::ActiveTz::Named(chrono_tz::UTC);
    a.json = true;
    a.offline = true;

    let mut cache = oronzo::usage::cache::Cache::default();
    let rows = parse::parse_all_cached(&fixtures(), &mut cache);
    let deduped = dedup::dedup(rows);
    let filtered = aggregate::filter(deduped, &a);
    let p = pricing::Pricing::bundled();
    let report = aggregate::aggregate_daily(filtered, &a, &p);
    let json = render::to_json(&report);
    assert!(json.contains("\"label\": \"2026-05-07\""));
    assert!(json.contains("\"input\": 300"), "got: {json}");
}

#[test]
fn usage_binary_help_runs() {
    let exe = env!("CARGO_BIN_EXE_oronzo");
    let out = Command::new(exe).args(["usage", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("oronzo usage"));
}
