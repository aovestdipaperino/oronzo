# `oronzo usage` — design

**Status:** approved (brainstorming)
**Date:** 2026-05-15
**Owner:** enzol

## Goal

Add a `usage` command to oronzo that ports the most useful parts of [ccusage](https://github.com/ryoppippi/ccusage) — token and USD-cost reporting over Claude Code's local JSONL session files — into the existing Rust CLI.

In-scope reports: `daily`, `weekly`, `monthly`, `session`, `blocks`. Statusline and MCP server modes are explicit non-goals.

## Command surface

```
oronzo usage                    # alias for `oronzo usage daily`
oronzo usage daily   [flags]
oronzo usage weekly  [flags]
oronzo usage monthly [flags]
oronzo usage session [flags]
oronzo usage blocks  [flags]
oronzo usage -h | --help
```

### Flags (all reports)

| Flag | Meaning |
|---|---|
| `--since YYYYMMDD` | inclusive lower bound (also accepts `YYYY-MM-DD`) |
| `--until YYYYMMDD` | inclusive upper bound (also accepts `YYYY-MM-DD`) |
| `--project <substr>` | filter by `cwd` substring match |
| `--breakdown` | per-model rows inside each bucket |
| `--instances` | (daily/weekly/monthly only) group buckets by project |
| `--timezone <tz>` | IANA name; defaults to system local |
| `--json` | machine-readable output |
| `--offline` | skip pricing refresh; use bundled snapshot |
| `--debug` | print parse stats, dedup counts, cache hits to stderr |

### Flags (`blocks` only)

| Flag | Meaning |
|---|---|
| `--active` | only the currently-open 5-hour window |
| `--recent N` | last `N` completed windows (default 10) |

Flag parsing is hand-rolled in `usage::args` to match the existing style of `mv.rs` and `switch.rs`. Unknown flags error with a hint. No `clap` dependency.

## Architecture

New module tree under `src/usage/`:

| File | Responsibility |
|---|---|
| `usage/mod.rs` | Entry point `pub fn run(args: &[String])`; dispatches subcommands; owns top-level error handling. |
| `usage/args.rs` | Parses subcommand + flags into a `UsageArgs` struct. |
| `usage/parse.rs` | Streams JSONL files into `UsageRow` records (one per assistant message with a `usage` block). |
| `usage/dedup.rs` | `(message_id, request_id)` HashSet filter applied before aggregation. |
| `usage/pricing.rs` | Loads bundled pricing, refreshes from LiteLLM once per day, supports `--offline`. |
| `usage/aggregate.rs` | Per-report bucketing into a generic `Report` value. |
| `usage/render.rs` | `comfy-table` for human output; `serde_json` for `--json`. |
| `usage/cache.rs` | Per-file usage-row cache at `~/.cache/oronzo/usage.json`. |

### Shared refactors

- Extract `discover_sessions` and `get_claude_dir` from `main.rs` into a new `src/sessions.rs` module. Both `usage::parse` and `mv.rs` need them; today they live only in `main.rs`.
- Rename `main.rs::usage_text()` → `help_text()` and `main.rs::usage()` → `help()` to free the word `usage` for the new command. The literal string `"Usage:"` inside the help body is unaffected.
- Add `usage   [report] [flags]` to the top-level help output.

## Data pipeline

Single pass per invocation:

1. **Discover** — reuse `sessions::discover()` from the shared module.
2. **Parse** — for each `.jsonl`, iterate lines. Keep rows where `type == "assistant"` and `message.usage` exists. Extract: `timestamp`, `message.model`, `message.id`, `requestId`, `message.usage.{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}`, plus `cwd` and `sessionId`. Decode `timestamp` once into `chrono::DateTime<Utc>`.
3. **Dedup** — global `HashSet<(String, String)>` keyed on `(message_id, request_id)`. Drop rows whose key was already seen. Rows missing either field pass through (rare; older logs).
4. **Filter** — apply `--since` / `--until` (parsed in the active timezone) and `--project <substr>`.
5. **Aggregate** — pass surviving rows to the report-specific bucketer:
   - `daily`: `date in tz → totals`
   - `weekly`: `ISO week (year + week number in tz) → totals`
   - `monthly`: `YYYY-MM in tz → totals`
   - `session`: `(project, sessionId) → totals + first/last timestamp`
   - `blocks`: 5-hour rolling-window algorithm; see §"Blocks algorithm" below.
6. **Cost** — for each bucket, walk per-model token totals and multiply against `pricing.json` rates (`input_cost_per_token`, `output_cost_per_token`, `cache_creation_input_token_cost`, `cache_read_input_token_cost`). When `--breakdown` is set, emit one row per `(bucket, model)`.
7. **Render** — `comfy-table` for humans; `serde_json::to_string_pretty` for `--json`.

### Usage cache

File: `~/.cache/oronzo/usage.json`. Schema: `{ path → { mtime: f64, rows: Vec<UsageRow> } }`. Distinct from the existing `~/.cache/claude-search/index.json`; different shape, different consumer.

On startup, re-parse only files whose mtime differs from the cached entry. Dedup operates on the union of cached and freshly-parsed rows so corrections to one file are picked up immediately.

## Pricing strategy

Source: LiteLLM's `model_prices_and_context_window.json` (raw GitHub URL). Ship a snapshot at `src/usage/pricing.json` checked into the repo.

Lifecycle:

1. **Build-time:** `build.rs` reads `src/usage/pricing.json` and exposes it via `include_str!`. If a future `build.rs` enhancement wants to refresh the snapshot during build, the download is best-effort and the build never fails because of it.
2. **Runtime:** if `~/.cache/oronzo/pricing.json` exists and its mtime is from the current calendar day (system local), use it. Otherwise spawn a 2-second `ureq` GET; on success, atomically replace the cache and use the new copy; on failure, silently fall through to the bundled snapshot.
3. `--offline` short-circuits straight to the bundled snapshot.
4. **Missing models:** model IDs not present in the pricing file (e.g. a model newer than the snapshot) compute as `cost = 0` with an `(missing pricing)` annotation in non-JSON output. JSON output sets `"cost_usd": null` for those rows.

## Blocks algorithm

Claude's subscription rate limit resets every 5 hours from the first message of a window. Replicate ccusage's logic:

1. Sort all (deduped) rows by timestamp.
2. The first row opens a window starting at `floor_to_hour(timestamp)`. The window stays open until either 5 hours elapse OR a gap of ≥5 hours occurs between consecutive rows, whichever comes first.
3. A ≥5-hour gap closes the current window. The next row after the gap opens a new window.
4. Each window aggregates its rows: total tokens, total cost, peak per-minute burn rate (max tokens in any single minute inside the window), and the set of models used.

Output columns: `Started · Ended · Duration · Models · Tokens · Cost · Status`. `Status` is `active` if the window's end is in the future, `closed` otherwise. `--active` keeps only the active window; `--recent N` keeps the last `N` closed windows plus the active one.

## Dependencies

New Cargo dependencies:

- `comfy-table` (~1.6) — bordered tables, color support.
- `chrono` (~0.4) with `clock` feature — timezone-aware date math.
- `chrono-tz` (~0.10) — IANA timezone names for `--timezone`.

Pricing refresh reuses the existing `ureq` dep.

## Testing

Fixtures under `tests/fixtures/usage/`:

- A baseline `proj_a/session1.jsonl` with known token totals.
- A dedup case: same `message.id` + `requestId` appearing in two different JSONL files.
- A row at 23:55 UTC (and a paired row at 00:05 UTC) for timezone-bucketing tests.
- A pair of rows separated by ≥5 hours to exercise blocks window-closing.

Unit tests (in `usage/*` modules):

- Dedup correctly drops duplicates and preserves singletons.
- `--since` / `--until` inclusive boundary behavior in the configured timezone.
- Daily / weekly / monthly bucketing against fixture timestamps.
- Session aggregation: correct `(project, sessionId)` keying and first/last timestamps.
- Blocks: 5-hour window opening, closing on gap, closing on duration, active vs closed status.
- Cost computation against a pinned pricing snapshot in `tests/fixtures/pricing.json`.

Integration test (in `tests/`):

- Invoke `oronzo usage daily --since … --json` against the fixture tree and assert the parsed JSON output.

## Non-goals

- Statusline mode.
- MCP server mode.
- Live tail / watch mode.
- Per-tool or per-MCP cost breakdown.
- Cost projections / forecasting.
