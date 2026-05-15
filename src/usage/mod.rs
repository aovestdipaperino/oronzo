pub mod aggregate;
pub mod args;
pub mod cache;
pub mod dedup;
pub mod parse;
pub mod pricing;
pub mod render;

use crate::sessions;
use std::process;

pub fn run(argv: &[String]) {
    if argv.first().map(|s| s.as_str()) == Some("-h")
        || argv.first().map(|s| s.as_str()) == Some("--help")
    {
        eprint!("{}", help());
        return;
    }

    let parsed = match args::parse(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("usage: {e}\n\n{}", help());
            process::exit(2);
        }
    };

    let claude_dir = sessions::claude_dir();
    let cache_path = cache::Cache::default_path();
    let mut cache = cache::Cache::load(cache_path);

    let raw_rows = parse::parse_all_cached(&claude_dir, &mut cache);
    if parsed.debug {
        eprintln!("usage: parsed {} raw rows", raw_rows.len());
    }
    let deduped = dedup::dedup(raw_rows);
    if parsed.debug {
        eprintln!("usage: {} rows after dedup", deduped.len());
    }
    let filtered = aggregate::filter(deduped, &parsed);

    let pricing = pricing::Pricing::load(parsed.offline);
    let base = match parsed.report {
        args::Report::Daily   => aggregate::aggregate_daily(filtered.clone(), &parsed, &pricing),
        args::Report::Weekly  => aggregate::aggregate_weekly(filtered.clone(), &parsed, &pricing),
        args::Report::Monthly => aggregate::aggregate_monthly(filtered.clone(), &parsed, &pricing),
        args::Report::Session => aggregate::aggregate_session(filtered.clone(), &parsed, &pricing),
        args::Report::Blocks  => {
            let r = aggregate::aggregate_blocks(filtered.clone(), &parsed, &pricing);
            aggregate::blocks_filter(r, &parsed)
        }
    };

    let report = if parsed.breakdown {
        aggregate::apply_breakdown(base, filtered.clone(), &parsed, &pricing)
    } else if parsed.instances {
        aggregate::apply_instances(base, filtered, &parsed, &pricing)
    } else {
        base
    };

    if parsed.json {
        println!("{}", render::to_json(&report));
    } else {
        println!("{}", render::to_table(&report));
    }
}

pub fn help() -> String {
    "\
oronzo usage: Token and cost reports across Claude Code sessions.

Usage:
  oronzo usage                  alias for `oronzo usage daily`
  oronzo usage daily   [flags]
  oronzo usage weekly  [flags]
  oronzo usage monthly [flags]
  oronzo usage session [flags]
  oronzo usage blocks  [flags]

Flags (all reports):
  --since YYYYMMDD       inclusive lower bound
  --until YYYYMMDD       inclusive upper bound
  --project <substr>     filter by cwd substring
  --breakdown            per-model rows inside each bucket
  --instances            (daily/weekly/monthly) split by project
  --timezone <IANA>      e.g. America/Los_Angeles (default: system local)
  --json                 machine-readable output
  --offline              skip pricing refresh; use bundled snapshot
  --debug                print parse/dedup stats to stderr

Blocks-only:
  --active               only the open 5-hour window
  --recent N             keep the last N closed windows (default 10)
"
    .to_string()
}
