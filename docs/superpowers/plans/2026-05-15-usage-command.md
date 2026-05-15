# `oronzo usage` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `usage` subcommand to oronzo that aggregates Claude Code token usage and USD cost across `~/.claude/projects/**/*.jsonl`, with `daily`, `weekly`, `monthly`, `session`, and `blocks` reports and ccusage-compatible filtering flags.

**Architecture:** New `src/usage/` module tree (args / parse / dedup / pricing / aggregate / render / cache) plus a small shared `src/sessions.rs` extracted from `main.rs`. Single-pass pipeline: discover → parse → dedup → filter → aggregate → cost → render. Pricing snapshot is embedded at build time, optionally refreshed daily from LiteLLM at runtime.

**Tech Stack:** Rust 2024 edition. New deps: `comfy-table`, `chrono` (with `clock`), `chrono-tz`. Existing: `serde`, `serde_json`, `dirs`, `ureq`.

**Spec:** [`docs/superpowers/specs/2026-05-15-usage-command-design.md`](../specs/2026-05-15-usage-command-design.md)

---

## Type contracts (referenced by multiple tasks)

These types are introduced in their own tasks. Listed here so later tasks can reference them without re-defining.

```rust
// src/usage/parse.rs
pub struct UsageRow {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub model: String,
    pub project: String,       // cwd
    pub session_id: String,
    pub message_id: String,
    pub request_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

// src/usage/pricing.rs
pub struct ModelPricing {
    pub input: f64,
    pub output: f64,
    pub cache_creation: f64,
    pub cache_read: f64,
}
pub struct Pricing { pub models: std::collections::HashMap<String, ModelPricing> }

// src/usage/args.rs
pub enum Report { Daily, Weekly, Monthly, Session, Blocks }
pub struct UsageArgs {
    pub report: Report,
    pub since: Option<chrono::NaiveDate>,
    pub until: Option<chrono::NaiveDate>,
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
pub enum ActiveTz {
    Named(chrono_tz::Tz),
    Local,
}

// src/usage/aggregate.rs
pub struct Bucket {
    pub label: String,
    pub project: Option<String>,
    pub model: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub cost_usd: Option<f64>,
    pub first: chrono::DateTime<chrono::Utc>,
    pub last: chrono::DateTime<chrono::Utc>,
}
pub struct ReportData {
    pub kind: ReportKind,
    pub buckets: Vec<Bucket>,
}
pub enum ReportKind { Daily, Weekly, Monthly, Session, Blocks }
```

---

### Task 1: Bootstrap — add deps and scaffold module files

**Files:**
- Modify: `Cargo.toml`
- Create: `src/sessions.rs`
- Create: `src/usage/mod.rs`
- Create: `src/usage/args.rs`
- Create: `src/usage/parse.rs`
- Create: `src/usage/dedup.rs`
- Create: `src/usage/pricing.rs`
- Create: `src/usage/aggregate.rs`
- Create: `src/usage/render.rs`
- Create: `src/usage/cache.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add the new dependencies**

Edit `Cargo.toml`, add under `[dependencies]`:

```toml
comfy-table = "7"
chrono = { version = "0.4", default-features = false, features = ["clock", "serde", "std"] }
chrono-tz = "0.10"
```

- [ ] **Step 2: Create empty module files**

Create each of the following with this exact one-line content (replace `MODULE` with the module name):

```rust
// placeholder
```

Files to create with the placeholder line:
- `src/sessions.rs`
- `src/usage/mod.rs`
- `src/usage/args.rs`
- `src/usage/parse.rs`
- `src/usage/dedup.rs`
- `src/usage/pricing.rs`
- `src/usage/aggregate.rs`
- `src/usage/render.rs`
- `src/usage/cache.rs`

- [ ] **Step 3: Register the modules in `src/main.rs`**

Add at the top of `src/main.rs`, just after the existing `mod` lines:

```rust
mod sessions;
mod usage;
```

In `src/usage/mod.rs`, replace the placeholder with:

```rust
pub mod aggregate;
pub mod args;
pub mod cache;
pub mod dedup;
pub mod parse;
pub mod pricing;
pub mod render;
```

- [ ] **Step 4: Verify the project still compiles**

Run: `cargo build`
Expected: clean build, only "unused" warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/sessions.rs src/usage/
git commit -m "feat(usage): scaffold module tree and dependencies"
```

---

### Task 2: Extract `sessions` module from `main.rs`

Pull `discover_sessions`, `SessionFile`, and `get_claude_dir` out of `main.rs` so the new usage parser can reuse them.

**Files:**
- Modify: `src/sessions.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Move the discovery code into `sessions.rs`**

Replace `src/sessions.rs` contents with:

```rust
use std::fs;
use std::path::{Path, PathBuf};

pub struct SessionFile {
    pub id: String,
    pub path: PathBuf,
}

pub fn claude_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let candidate = PathBuf::from(appdata).join("Claude").join("projects");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("projects")
}

pub fn discover(claude_dir: &Path) -> Vec<SessionFile> {
    let mut sessions = Vec::new();
    let Ok(projects) = fs::read_dir(claude_dir) else {
        return sessions;
    };
    for project_entry in projects.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(files) = fs::read_dir(&project_path) else {
            continue;
        };
        for file_entry in files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                sessions.push(SessionFile { id, path });
            }
        }
    }
    sessions.sort_by(|a, b| a.path.cmp(&b.path));
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_fixture_sessions() {
        let fixture_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let sessions = discover(&fixture_dir);
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|s| s.id == "session1"));
        assert!(sessions.iter().any(|s| s.id == "session2"));
    }
}
```

- [ ] **Step 2: Update `src/main.rs` to use it**

In `src/main.rs`:
- Delete the `SessionFile` struct (lines around 231-234).
- Delete the `get_claude_dir` function (lines around 236-249).
- Delete the `discover_sessions` function (lines around 251-278).
- Delete the existing `test_discover_sessions` test in the `tests` mod (already moved to `sessions.rs`).
- Replace remaining call sites:
  - `let claude_dir = get_claude_dir();` → `let claude_dir = sessions::claude_dir();`
  - `let session_files = discover_sessions(&claude_dir);` → `let session_files = sessions::discover(&claude_dir);`

- [ ] **Step 3: Run the test suite**

Run: `cargo test`
Expected: all tests pass, including `sessions::tests::discovers_fixture_sessions`.

- [ ] **Step 4: Commit**

```bash
git add src/sessions.rs src/main.rs
git commit -m "refactor: extract session discovery into shared sessions module"
```

---

### Task 3: Rename `usage_text` / `usage` helpers in `main.rs`

Free the word `usage` for the new command. The literal "Usage:" string in the help body is unchanged.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Rename the function definitions**

In `src/main.rs`:
- `fn usage_text() -> String` → `fn help_text() -> String`
- `fn usage() -> String` → `fn help() -> String`

- [ ] **Step 2: Rename the call sites**

In `src/main.rs`:
- `eprint!("{}", usage());` → `eprint!("{}", help());`
- `eprint!("{}", usage_text());` → `eprint!("{}", help_text());`

- [ ] **Step 3: Verify build and tests**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor: rename help helpers to free the word usage"
```

---

### Task 4: Pricing snapshot file and bundled loader

**Files:**
- Create: `src/usage/pricing.json`
- Modify: `src/usage/pricing.rs`

- [ ] **Step 1: Create the bundled pricing snapshot**

Create `src/usage/pricing.json` with these entries (synthetic round values; the maintainer will refresh from LiteLLM later — task 27 documents this):

```json
{
  "claude-haiku-4-5-20251001": {
    "input_cost_per_token": 0.000001,
    "output_cost_per_token": 0.000005,
    "cache_creation_input_token_cost": 0.00000125,
    "cache_read_input_token_cost": 0.0000001
  },
  "claude-sonnet-4-6": {
    "input_cost_per_token": 0.000003,
    "output_cost_per_token": 0.000015,
    "cache_creation_input_token_cost": 0.00000375,
    "cache_read_input_token_cost": 0.0000003
  },
  "claude-opus-4-7": {
    "input_cost_per_token": 0.000015,
    "output_cost_per_token": 0.000075,
    "cache_creation_input_token_cost": 0.00001875,
    "cache_read_input_token_cost": 0.0000015
  }
}
```

- [ ] **Step 2: Write the failing test**

Replace `src/usage/pricing.rs` with:

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelPricing {
    #[serde(rename = "input_cost_per_token", default)]
    pub input: f64,
    #[serde(rename = "output_cost_per_token", default)]
    pub output: f64,
    #[serde(rename = "cache_creation_input_token_cost", default)]
    pub cache_creation: f64,
    #[serde(rename = "cache_read_input_token_cost", default)]
    pub cache_read: f64,
}

#[derive(Debug, Default, Clone)]
pub struct Pricing {
    pub models: HashMap<String, ModelPricing>,
}

const BUNDLED: &str = include_str!("pricing.json");

impl Pricing {
    pub fn bundled() -> Self {
        let models: HashMap<String, ModelPricing> =
            serde_json::from_str(BUNDLED).unwrap_or_default();
        Pricing { models }
    }

    pub fn lookup(&self, model: &str) -> Option<&ModelPricing> {
        self.models.get(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_loads_sonnet_46() {
        let p = Pricing::bundled();
        let m = p.lookup("claude-sonnet-4-6").expect("model present");
        assert!((m.input - 0.000003).abs() < 1e-12);
        assert!((m.output - 0.000015).abs() < 1e-12);
        assert!((m.cache_creation - 0.00000375).abs() < 1e-12);
        assert!((m.cache_read - 0.0000003).abs() < 1e-12);
    }

    #[test]
    fn bundled_returns_none_for_unknown_model() {
        let p = Pricing::bundled();
        assert!(p.lookup("nonexistent-model").is_none());
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test usage::pricing`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/usage/pricing.json src/usage/pricing.rs
git commit -m "feat(usage): embed bundled pricing snapshot"
```

---

### Task 5: Runtime pricing cache and refresh

Pricing is refreshed at most once per calendar day from LiteLLM; `--offline` skips the network entirely.

**Files:**
- Modify: `src/usage/pricing.rs`

- [ ] **Step 1: Write the failing test for offline-only behavior**

Append to the `tests` mod in `src/usage/pricing.rs`:

```rust
    #[test]
    fn load_offline_returns_bundled() {
        let p = Pricing::load(true);
        assert!(p.lookup("claude-sonnet-4-6").is_some());
    }
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::pricing::tests::load_offline_returns_bundled`
Expected: FAIL — no `load` method yet.

- [ ] **Step 3: Implement `load`, the daily cache, and refresh**

Append to `src/usage/pricing.rs` (above `#[cfg(test)]`):

```rust
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LITELLM_URL: &str = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oronzo")
        .join("pricing.json")
}

fn mtime_is_today(path: &std::path::Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    let Ok(mtime_secs) = mtime.duration_since(UNIX_EPOCH) else {
        return false;
    };
    let Ok(now_secs) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    const ONE_DAY: u64 = 86_400;
    (now_secs.as_secs() / ONE_DAY) == (mtime_secs.as_secs() / ONE_DAY)
}

fn parse_str(s: &str) -> Pricing {
    let models = serde_json::from_str(s).unwrap_or_default();
    Pricing { models }
}

fn try_fetch_and_cache() -> Option<Pricing> {
    let resp = ureq::get(LITELLM_URL)
        .config()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .call()
        .ok()?;
    let body = resp.into_body().read_to_string().ok()?;
    let parsed = parse_str(&body);
    if parsed.models.is_empty() {
        return None;
    }
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, &body).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
    Some(parsed)
}

impl Pricing {
    pub fn load(offline: bool) -> Self {
        if offline {
            return Pricing::bundled();
        }
        let path = cache_path();
        if mtime_is_today(&path) {
            if let Ok(body) = fs::read_to_string(&path) {
                let parsed = parse_str(&body);
                if !parsed.models.is_empty() {
                    return parsed;
                }
            }
        }
        if let Some(fresh) = try_fetch_and_cache() {
            return fresh;
        }
        Pricing::bundled()
    }
}
```

- [ ] **Step 4: Run the test (expected to pass)**

Run: `cargo test usage::pricing`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/pricing.rs
git commit -m "feat(usage): add daily pricing refresh with offline fallback"
```

---

### Task 6: Cost computation helper

**Files:**
- Modify: `src/usage/pricing.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` mod in `src/usage/pricing.rs`:

```rust
    #[test]
    fn compute_cost_multiplies_each_token_class() {
        let p = Pricing::bundled();
        let cost = p.compute_cost("claude-sonnet-4-6", 1000, 500, 200, 100);
        // input: 1000 * 0.000003 = 0.003
        // output: 500 * 0.000015 = 0.0075
        // cache_creation: 200 * 0.00000375 = 0.00075
        // cache_read: 100 * 0.0000003 = 0.00003
        // total = 0.01128
        let expected = 0.01128;
        assert!((cost.unwrap() - expected).abs() < 1e-9, "got {:?}", cost);
    }

    #[test]
    fn compute_cost_returns_none_for_unknown_model() {
        let p = Pricing::bundled();
        assert!(p.compute_cost("not-a-model", 1, 1, 1, 1).is_none());
    }
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::pricing::tests::compute_cost`
Expected: FAIL — no method `compute_cost`.

- [ ] **Step 3: Implement `compute_cost`**

Inside `impl Pricing` in `src/usage/pricing.rs`, add:

```rust
    pub fn compute_cost(
        &self,
        model: &str,
        input: u64,
        output: u64,
        cache_creation: u64,
        cache_read: u64,
    ) -> Option<f64> {
        let m = self.lookup(model)?;
        Some(
            (input as f64) * m.input
                + (output as f64) * m.output
                + (cache_creation as f64) * m.cache_creation
                + (cache_read as f64) * m.cache_read,
        )
    }
```

- [ ] **Step 4: Run the test**

Run: `cargo test usage::pricing`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/pricing.rs
git commit -m "feat(usage): compute USD cost per (model, token-class) tuple"
```

---

### Task 7: Parse a single JSONL into `UsageRow`s

**Files:**
- Create: `tests/fixtures/usage/proj_a/session_a1.jsonl`
- Modify: `src/usage/parse.rs`

- [ ] **Step 1: Create the parse fixture**

Create `tests/fixtures/usage/proj_a/session_a1.jsonl` with these two lines (exact content, one per line):

```
{"type":"user","cwd":"/tmp/proj_a","sessionId":"sess-a","message":{"content":"hi"}}
{"type":"assistant","cwd":"/tmp/proj_a","sessionId":"sess-a","requestId":"req-001","timestamp":"2026-05-07T10:00:00Z","message":{"id":"msg-001","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}}
```

- [ ] **Step 2: Write the failing test**

Replace `src/usage/parse.rs` with:

```rust
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
        // The fixture has one user row and one assistant row → exactly 1 usage row.
        let rows = extract_rows(&fixture("tests/fixtures/usage/proj_a/session_a1.jsonl"));
        assert_eq!(rows.len(), 1);
    }
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test usage::parse`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/usage/parse.rs tests/fixtures/usage/proj_a/session_a1.jsonl
git commit -m "feat(usage): parse assistant usage rows from a JSONL file"
```

---

### Task 8: Parse all sessions across discovery

**Files:**
- Create: `tests/fixtures/usage/proj_b/session_b1.jsonl`
- Modify: `src/usage/parse.rs`

- [ ] **Step 1: Add a second-project fixture**

Create `tests/fixtures/usage/proj_b/session_b1.jsonl`:

```
{"type":"assistant","cwd":"/tmp/proj_b","sessionId":"sess-b","requestId":"req-100","timestamp":"2026-05-07T11:00:00Z","message":{"id":"msg-100","model":"claude-haiku-4-5-20251001","usage":{"input_tokens":200,"output_tokens":80}}}
```

- [ ] **Step 2: Write the failing test**

Append to the `tests` mod in `src/usage/parse.rs`:

```rust
    #[test]
    fn parse_all_walks_every_project() {
        let dir = fixture("tests/fixtures/usage");
        let rows = parse_all(&dir);
        assert_eq!(rows.len(), 2);
        let projects: std::collections::HashSet<&str> =
            rows.iter().map(|r| r.project.as_str()).collect();
        assert!(projects.contains("/tmp/proj_a"));
        assert!(projects.contains("/tmp/proj_b"));
    }
```

- [ ] **Step 3: Run the test (expected to fail)**

Run: `cargo test usage::parse::tests::parse_all_walks_every_project`
Expected: FAIL — no function `parse_all`.

- [ ] **Step 4: Implement `parse_all`**

Append to `src/usage/parse.rs` (above the `#[cfg(test)]` block):

```rust
use crate::sessions;

pub fn parse_all(claude_dir: &Path) -> Vec<UsageRow> {
    let files = sessions::discover(claude_dir);
    let mut rows = Vec::new();
    for sf in files {
        rows.extend(extract_rows(&sf.path));
    }
    rows
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test usage::parse`
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/usage/parse.rs tests/fixtures/usage/proj_b/session_b1.jsonl
git commit -m "feat(usage): walk all discovered sessions into a single row vec"
```

---

### Task 9: Dedup by `(message_id, request_id)`

**Files:**
- Create: `tests/fixtures/usage/proj_a/session_a1_dup.jsonl`
- Modify: `src/usage/dedup.rs`

- [ ] **Step 1: Add the duplicate-message fixture**

Create `tests/fixtures/usage/proj_a/session_a1_dup.jsonl` (same `message.id` and `requestId` as `session_a1.jsonl`):

```
{"type":"assistant","cwd":"/tmp/proj_a","sessionId":"sess-a-fork","requestId":"req-001","timestamp":"2026-05-07T10:00:00Z","message":{"id":"msg-001","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50}}}
```

- [ ] **Step 2: Write the failing test**

Replace `src/usage/dedup.rs` with:

```rust
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
```

- [ ] **Step 3: Run the test**

Run: `cargo test usage::dedup`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/usage/dedup.rs tests/fixtures/usage/proj_a/session_a1_dup.jsonl
git commit -m "feat(usage): dedup rows by (message_id, request_id)"
```

---

### Task 10: Usage row cache

**Files:**
- Modify: `src/usage/cache.rs`
- Modify: `src/usage/parse.rs` (derive `Serialize`/`Deserialize` on `UsageRow`)

- [ ] **Step 1: Make `UsageRow` serializable**

In `src/usage/parse.rs`, change the `UsageRow` declaration:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageRow {
```

- [ ] **Step 2: Write the failing test**

Replace `src/usage/cache.rs` with:

```rust
use crate::usage::parse::UsageRow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub mtime: f64,
    pub rows: Vec<UsageRow>,
}

#[derive(Default)]
pub struct Cache {
    pub path: PathBuf,
    pub entries: HashMap<String, Entry>,
}

impl Cache {
    pub fn default_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oronzo")
            .join("usage.json")
    }

    pub fn load(path: PathBuf) -> Self {
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Cache { path, entries }
    }

    pub fn get(&self, key: &str, mtime: f64) -> Option<&Entry> {
        self.entries
            .get(key)
            .filter(|e| (e.mtime - mtime).abs() < 1e-6)
    }

    pub fn set(&mut self, key: String, entry: Entry) {
        self.entries.insert(key, entry);
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&self.entries) {
            let _ = fs::write(&self.path, json);
        }
    }
}

pub fn file_mtime(p: &Path) -> f64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    fn row() -> UsageRow {
        UsageRow {
            timestamp: Utc::now(),
            model: "m".into(),
            project: "p".into(),
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
    fn round_trips_through_disk() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("usage.json");
        let mut c = Cache::load(path.clone());
        c.set(
            "/file/a".into(),
            Entry {
                mtime: 1.0,
                rows: vec![row()],
            },
        );
        c.save();
        let loaded = Cache::load(path);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries["/file/a"].rows.len(), 1);
    }

    #[test]
    fn get_returns_none_on_mtime_mismatch() {
        let mut c = Cache::default();
        c.set(
            "/file/a".into(),
            Entry {
                mtime: 1.0,
                rows: vec![],
            },
        );
        assert!(c.get("/file/a", 1.0).is_some());
        assert!(c.get("/file/a", 2.0).is_none());
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test usage::cache`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/usage/cache.rs src/usage/parse.rs
git commit -m "feat(usage): per-file row cache with mtime invalidation"
```

---

### Task 11: Args parser — subcommand + date flags

**Files:**
- Modify: `src/usage/args.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/usage/args.rs` with:

```rust
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
            _ => {
                return Err(format!("unknown flag: {a}"));
            }
        }
    }
    Ok(out)
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
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test usage::args`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/usage/args.rs
git commit -m "feat(usage): parse subcommand and --since/--until flags"
```

---

### Task 12: Args parser — remaining flags

**Files:**
- Modify: `src/usage/args.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` mod in `src/usage/args.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests (expected to fail)**

Run: `cargo test usage::args`
Expected: the new tests FAIL — unknown flags.

- [ ] **Step 3: Extend `parse` to handle every flag**

In `src/usage/args.rs`, replace the body of `parse` with:

```rust
pub fn parse(args: &[String]) -> Result<UsageArgs, String> {
    let mut out = UsageArgs::default();
    let mut i = 0;
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test usage::args`
Expected: 9 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/args.rs
git commit -m "feat(usage): parse all remaining flags with per-report validation"
```

---

### Task 13: Timezone-aware date helpers

**Files:**
- Modify: `src/usage/args.rs`

`ActiveTz::date_of` and `ActiveTz::ymd_label` are used by every aggregator.

- [ ] **Step 1: Write the failing test**

Append to the `tests` mod in `src/usage/args.rs`:

```rust
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
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::args::tests::date_of_handles_timezone_shift_across_midnight`
Expected: FAIL — no `date_of` method.

- [ ] **Step 3: Implement the helpers**

Append to `src/usage/args.rs` (above the `#[cfg(test)]` block):

```rust
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
```

- [ ] **Step 4: Run the test**

Run: `cargo test usage::args`
Expected: 10 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/args.rs
git commit -m "feat(usage): timezone-aware date/week/month label helpers"
```

---

### Task 14: Filter rows by date and project

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/usage/aggregate.rs` with:

```rust
use crate::usage::args::{ActiveTz, UsageArgs};
use crate::usage::parse::UsageRow;
use chrono::{DateTime, Utc};

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
    use chrono::NaiveDate;

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
```

- [ ] **Step 2: Run the tests**

Run: `cargo test usage::aggregate`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): filter rows by date and project substring"
```

---

### Task 15: Bucket type and daily aggregation

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` mod in `src/usage/aggregate.rs`:

```rust
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
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::aggregate::tests::daily_buckets_rows_by_calendar_day`
Expected: FAIL — `aggregate_daily` not defined.

- [ ] **Step 3: Add the bucket types and daily aggregator**

Append to `src/usage/aggregate.rs` (above the `#[cfg(test)]` block):

```rust
use crate::usage::pricing::Pricing;
use serde::Serialize;
use std::collections::BTreeMap;

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
```

- [ ] **Step 4: Run the test**

Run: `cargo test usage::aggregate`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): daily aggregation with timezone-aware bucketing"
```

---

### Task 16: Weekly and monthly aggregation

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` mod in `src/usage/aggregate.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests (expected to fail)**

Run: `cargo test usage::aggregate`
Expected: 2 new tests FAIL — functions not defined.

- [ ] **Step 3: Implement the aggregators**

Append to `src/usage/aggregate.rs` (above the `#[cfg(test)]` block):

```rust
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test usage::aggregate`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): weekly and monthly aggregation"
```

---

### Task 17: Session aggregation

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` mod in `src/usage/aggregate.rs`:

```rust
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
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::aggregate::tests::session_buckets_by_project_session_pair`
Expected: FAIL.

- [ ] **Step 3: Implement `aggregate_session`**

Append to `src/usage/aggregate.rs` (above the `#[cfg(test)]` block):

```rust
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
```

- [ ] **Step 4: Run the test**

Run: `cargo test usage::aggregate`
Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): per-session aggregation keyed on (project, session_id)"
```

---

### Task 18: Blocks aggregation — 5-hour windows

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` mod in `src/usage/aggregate.rs`:

```rust
    #[test]
    fn blocks_opens_one_window_within_five_hours() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let rows = vec![
            row("2026-05-07T10:15:00Z", "/p"),
            row("2026-05-07T13:00:00Z", "/p"),
        ];
        let report = aggregate_blocks(rows, &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 1);
    }

    #[test]
    fn blocks_closes_on_five_hour_gap() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let rows = vec![
            row("2026-05-07T10:00:00Z", "/p"),
            row("2026-05-07T15:30:00Z", "/p"), // 5h30m later
        ];
        let report = aggregate_blocks(rows, &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 2);
    }

    #[test]
    fn blocks_closes_on_window_duration() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        let rows = vec![
            row("2026-05-07T10:00:00Z", "/p"),
            row("2026-05-07T11:00:00Z", "/p"),
            row("2026-05-07T14:30:00Z", "/p"),
            row("2026-05-07T15:30:00Z", "/p"), // floor 10:00 + 5h ends at 15:00 → new window
        ];
        let report = aggregate_blocks(rows, &args, &crate::usage::pricing::Pricing::bundled());
        assert_eq!(report.buckets.len(), 2);
    }
```

- [ ] **Step 2: Run the tests (expected to fail)**

Run: `cargo test usage::aggregate::tests::blocks`
Expected: 3 tests FAIL.

- [ ] **Step 3: Implement `aggregate_blocks`**

Append to `src/usage/aggregate.rs` (above the `#[cfg(test)]` block):

```rust
use chrono::Duration;
use chrono::Timelike;

fn floor_to_hour(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.with_minute(0).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
}

pub fn aggregate_blocks(mut rows: Vec<UsageRow>, _args: &UsageArgs, pricing: &Pricing) -> ReportData {
    rows.sort_by_key(|r| r.timestamp);
    let window_len = Duration::hours(5);

    let mut buckets: Vec<Bucket> = Vec::new();
    let mut current: Option<(Acc, DateTime<Utc>, DateTime<Utc>)> = None;
    // (accumulator, window_start, last_row_timestamp)

    for r in rows {
        match &mut current {
            None => {
                let start = floor_to_hour(r.timestamp);
                let mut a = Acc::default();
                a.add(&r, pricing);
                current = Some((a, start, r.timestamp));
            }
            Some((acc, start, last)) => {
                let gap = r.timestamp - *last;
                let duration_full = r.timestamp - *start >= window_len;
                if gap >= window_len || duration_full {
                    // close current
                    let started = *start;
                    let ended = *last;
                    let mut bucket = std::mem::take(acc).into_bucket(format!(
                        "{} → {}",
                        started.to_rfc3339(),
                        ended.to_rfc3339()
                    ));
                    bucket.first = started;
                    bucket.last = ended;
                    buckets.push(bucket);
                    // open new
                    let new_start = floor_to_hour(r.timestamp);
                    let mut a = Acc::default();
                    a.add(&r, pricing);
                    current = Some((a, new_start, r.timestamp));
                } else {
                    acc.add(&r, pricing);
                    *last = r.timestamp;
                }
            }
        }
    }
    if let Some((acc, start, last)) = current {
        let mut bucket = acc.into_bucket(format!(
            "{} → {}",
            start.to_rfc3339(),
            last.to_rfc3339()
        ));
        bucket.first = start;
        bucket.last = last;
        buckets.push(bucket);
    }
    ReportData { kind: ReportKind::Blocks, buckets }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test usage::aggregate`
Expected: 9 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): 5-hour billing-block aggregation"
```

---

### Task 19: Blocks `--active` and `--recent` filtering

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` mod in `src/usage/aggregate.rs`:

```rust
    #[test]
    fn blocks_active_keeps_only_open_window() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        args.active = true;

        let now = Utc::now();
        let recent_ts = now - chrono::Duration::minutes(30);
        let old_ts = now - chrono::Duration::hours(20);

        let rows = vec![
            row(&old_ts.to_rfc3339(), "/p"),
            row(&recent_ts.to_rfc3339(), "/p"),
        ];

        let r = aggregate_blocks(rows, &args, &crate::usage::pricing::Pricing::bundled());
        let filtered = blocks_filter(r, &args);
        assert_eq!(filtered.buckets.len(), 1);
        // The kept window should be the one containing the recent timestamp.
        assert!(filtered.buckets[0].last >= recent_ts - chrono::Duration::seconds(1));
    }

    #[test]
    fn blocks_recent_n_caps_history() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        args.recent = 2;
        // Construct four windows by spacing rows far apart.
        let rows = vec![
            row("2026-01-01T10:00:00Z", "/p"),
            row("2026-01-02T10:00:00Z", "/p"),
            row("2026-01-03T10:00:00Z", "/p"),
            row("2026-01-04T10:00:00Z", "/p"),
        ];
        let r = aggregate_blocks(rows, &args, &crate::usage::pricing::Pricing::bundled());
        let filtered = blocks_filter(r, &args);
        assert_eq!(filtered.buckets.len(), 2);
    }
```

- [ ] **Step 2: Run the tests (expected to fail)**

Run: `cargo test usage::aggregate::tests::blocks_active_keeps_only_open_window`
Expected: FAIL — no `blocks_filter`.

- [ ] **Step 3: Implement `blocks_filter` and status helper**

Append to `src/usage/aggregate.rs` (above the `#[cfg(test)]` block):

```rust
pub fn block_is_active(b: &Bucket, now: DateTime<Utc>) -> bool {
    // A window is "active" if its closing time (first + 5h) is in the future
    // AND the most recent row was within 5h of now.
    let window_end = b.first + Duration::hours(5);
    window_end > now && (now - b.last) < Duration::hours(5)
}

pub fn blocks_filter(report: ReportData, args: &UsageArgs) -> ReportData {
    let now = Utc::now();
    let mut buckets = report.buckets;
    if args.active {
        buckets.retain(|b| block_is_active(b, now));
        return ReportData { kind: ReportKind::Blocks, buckets };
    }
    let active: Vec<Bucket> = buckets.iter().filter(|b| block_is_active(b, now)).cloned().collect();
    let mut closed: Vec<Bucket> = buckets.into_iter().filter(|b| !block_is_active(b, now)).collect();
    closed.sort_by_key(|b| b.first);
    let tail_start = closed.len().saturating_sub(args.recent);
    let kept_closed: Vec<Bucket> = closed.into_iter().skip(tail_start).collect();
    let mut out = kept_closed;
    out.extend(active);
    out.sort_by_key(|b| b.first);
    ReportData { kind: ReportKind::Blocks, buckets: out }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test usage::aggregate`
Expected: 11 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): --active and --recent filtering for blocks"
```

---

### Task 20: JSON renderer

**Files:**
- Modify: `src/usage/render.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/usage/render.rs` with:

```rust
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
```

- [ ] **Step 2: Run the test**

Run: `cargo test usage::render`
Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add src/usage/render.rs
git commit -m "feat(usage): JSON renderer"
```

---

### Task 21: Table renderer

**Files:**
- Modify: `src/usage/render.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` mod in `src/usage/render.rs`:

```rust
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
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::render::tests::table_includes_label_and_token_columns`
Expected: FAIL — table renderer is a stub.

- [ ] **Step 3: Implement `to_table`**

Replace the `to_table` body in `src/usage/render.rs`:

```rust
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
```

- [ ] **Step 4: Run the test**

Run: `cargo test usage::render`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/render.rs
git commit -m "feat(usage): bordered table renderer via comfy-table"
```

---

### Task 22: Breakdown rows and project sub-grouping

**Files:**
- Modify: `src/usage/aggregate.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` mod in `src/usage/aggregate.rs`:

```rust
    #[test]
    fn breakdown_emits_one_row_per_model_within_bucket() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        args.breakdown = true;
        args.offline = true;
        let rows = vec![
            { let mut r = row("2026-05-07T10:00:00Z","/p"); r.model="claude-sonnet-4-6".into(); r },
            { let mut r = row("2026-05-07T11:00:00Z","/p"); r.model="claude-haiku-4-5-20251001".into(); r },
        ];
        let report = aggregate_daily(rows.clone(), &args, &crate::usage::pricing::Pricing::bundled());
        let expanded = apply_breakdown(report, rows, &args);
        assert_eq!(expanded.buckets.len(), 2);
        let models: std::collections::HashSet<_> =
            expanded.buckets.iter().filter_map(|b| b.model.as_deref()).collect();
        assert!(models.contains("claude-sonnet-4-6"));
        assert!(models.contains("claude-haiku-4-5-20251001"));
    }

    #[test]
    fn instances_emits_one_row_per_project_within_bucket() {
        let mut args = UsageArgs::default();
        args.timezone = ActiveTz::Named(chrono_tz::UTC);
        args.instances = true;
        args.offline = true;
        let rows = vec![
            row("2026-05-07T10:00:00Z", "/proj/a"),
            row("2026-05-07T11:00:00Z", "/proj/b"),
        ];
        let report = aggregate_daily(rows.clone(), &args, &crate::usage::pricing::Pricing::bundled());
        let expanded = apply_instances(report, rows, &args);
        assert_eq!(expanded.buckets.len(), 2);
        let projects: std::collections::HashSet<_> =
            expanded.buckets.iter().filter_map(|b| b.project.as_deref()).collect();
        assert!(projects.contains("/proj/a"));
        assert!(projects.contains("/proj/b"));
    }
```

- [ ] **Step 2: Run the tests (expected to fail)**

Run: `cargo test usage::aggregate::tests::breakdown_emits_one_row_per_model_within_bucket usage::aggregate::tests::instances_emits_one_row_per_project_within_bucket`
Expected: 2 tests FAIL.

- [ ] **Step 3: Implement breakdown and instances**

Append to `src/usage/aggregate.rs` (above the `#[cfg(test)]` block):

```rust
fn label_for(report_kind: &ReportKind, tz: &ActiveTz, ts: DateTime<Utc>) -> String {
    match report_kind {
        ReportKind::Daily   => tz.ymd_label(ts),
        ReportKind::Weekly  => tz.iso_week_label(ts),
        ReportKind::Monthly => tz.ym_label(ts),
        ReportKind::Session => String::new(),
        ReportKind::Blocks  => String::new(),
    }
}

pub fn apply_breakdown(report: ReportData, rows: Vec<UsageRow>, args: &UsageArgs) -> ReportData {
    if !args.breakdown {
        return report;
    }
    let pricing = Pricing::load(args.offline);
    let mut acc: BTreeMap<(String, String), Acc> = BTreeMap::new();
    for r in rows {
        let label = label_for(&report.kind, &args.timezone, r.timestamp);
        let key = (label.clone(), r.model.clone());
        let entry = acc.entry(key).or_default();
        if entry.model.is_none() {
            entry.model = Some(r.model.clone());
        }
        entry.add(&r, &pricing);
    }
    let buckets = acc
        .into_iter()
        .map(|((label, _), a)| a.into_bucket(label))
        .collect();
    ReportData { kind: report.kind, buckets }
}

pub fn apply_instances(report: ReportData, rows: Vec<UsageRow>, args: &UsageArgs) -> ReportData {
    if !args.instances {
        return report;
    }
    let pricing = Pricing::load(args.offline);
    let mut acc: BTreeMap<(String, String), Acc> = BTreeMap::new();
    for r in rows {
        let label = label_for(&report.kind, &args.timezone, r.timestamp);
        let key = (label.clone(), r.project.clone());
        let entry = acc.entry(key).or_default();
        if entry.project.is_none() {
            entry.project = Some(r.project.clone());
        }
        entry.add(&r, &pricing);
    }
    let buckets = acc
        .into_iter()
        .map(|((label, _), a)| a.into_bucket(label))
        .collect();
    ReportData { kind: report.kind, buckets }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test usage::aggregate`
Expected: 13 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/aggregate.rs
git commit -m "feat(usage): --breakdown and --instances sub-grouping"
```

---

### Task 23: Cache-aware parse pipeline

Re-use cached rows for unchanged files, and freshly parse changed ones.

**Files:**
- Modify: `src/usage/parse.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` mod in `src/usage/parse.rs`:

```rust
    #[test]
    fn parse_all_cached_returns_same_rows_as_parse_all() {
        use crate::usage::cache::Cache;
        let dir = fixture("tests/fixtures/usage");
        let baseline = parse_all(&dir);
        let mut cache = Cache::default();
        let cached = parse_all_cached(&dir, &mut cache);
        assert_eq!(cached.len(), baseline.len());
    }
```

- [ ] **Step 2: Run the test (expected to fail)**

Run: `cargo test usage::parse::tests::parse_all_cached_returns_same_rows_as_parse_all`
Expected: FAIL — function not defined.

- [ ] **Step 3: Implement `parse_all_cached`**

Append to `src/usage/parse.rs` (above the `#[cfg(test)]` block):

```rust
use crate::usage::cache::{file_mtime, Cache, Entry};

pub fn parse_all_cached(claude_dir: &Path, cache: &mut Cache) -> Vec<UsageRow> {
    let files = sessions::discover(claude_dir);
    let mut rows = Vec::new();
    let mut dirty = false;
    for sf in files {
        let key = sf.path.to_string_lossy().to_string();
        let mtime = file_mtime(&sf.path);
        if let Some(entry) = cache.get(&key, mtime) {
            rows.extend(entry.rows.clone());
            continue;
        }
        let fresh = extract_rows(&sf.path);
        cache.set(key, Entry { mtime, rows: fresh.clone() });
        rows.extend(fresh);
        dirty = true;
    }
    if dirty {
        cache.save();
    }
    rows
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test usage::parse`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/usage/parse.rs
git commit -m "feat(usage): cache-aware parse pipeline"
```

---

### Task 24: `usage::run` entry point

Wire everything into one function and dispatch from `main.rs`.

**Files:**
- Modify: `src/usage/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement the entry point**

Replace `src/usage/mod.rs` with:

```rust
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
        aggregate::apply_breakdown(base, filtered.clone(), &parsed)
    } else if parsed.instances {
        aggregate::apply_instances(base, filtered, &parsed)
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
".to_string()
}
```

- [ ] **Step 2: Dispatch in `main.rs`**

In `src/main.rs`, inside the `match args[1].as_str()` block, add a new arm above `other =>`:

```rust
        "usage" => usage::run(&args[2..]),
```

- [ ] **Step 3: Verify the project builds**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: every test passes.

- [ ] **Step 5: Commit**

```bash
git add src/usage/mod.rs src/main.rs
git commit -m "feat(usage): wire end-to-end usage::run dispatch"
```

---

### Task 25: Add `usage` to top-level help

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update the help text**

In `src/main.rs::help_text`, add the `usage` line to the `Commands:` block. The full block should read:

```rust
Commands:
  search <query>       Search and resume sessions
  usage [report]       Token and cost reports (daily/weekly/monthly/session/blocks)
  account-switch       Interactive account switcher
  account-save         Save current account
  account-list         List saved accounts
  account-use <email>  Switch to a specific account
  mv <from> <to>       Move folder, keep sessions
  upgrade              Update to the latest version
```

- [ ] **Step 2: Smoke-test the help output**

Run: `cargo run -- --help 2>&1 | head -20`
Expected: the new `usage` line appears.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "docs(usage): surface new command in top-level help"
```

---

### Task 26: End-to-end integration test

**Files:**
- Create: `tests/usage_e2e.rs`

- [ ] **Step 1: Write the integration test**

Create `tests/usage_e2e.rs`:

```rust
use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/usage")
}

#[test]
fn usage_daily_json_against_fixtures() {
    // The binary uses the real ~/.claude/projects path; rather than mock that
    // here, we drive the public API directly the way `usage::run` does.
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
```

- [ ] **Step 2: Expose internal modules to integration tests**

To let `tests/usage_e2e.rs` import the modules, oronzo needs a library target. Add to `Cargo.toml` under `[package]` (or as a new `[lib]` section):

```toml
[lib]
name = "oronzo"
path = "src/main.rs"
```

Then in `src/main.rs`, expose the modules and the `main` body:

- Change `mod sessions;` → `pub mod sessions;`
- Change `mod usage;` → `pub mod usage;`
- Change `mod mv;` → `pub mod mv;`
- Change `mod switch;` → `pub mod switch;`
- Change `mod upgrade;` → `pub mod upgrade;`

- [ ] **Step 3: Run the integration test**

Run: `cargo test --test usage_e2e`
Expected: both tests pass.

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: everything passes (unit + integration).

- [ ] **Step 5: Commit**

```bash
git add tests/usage_e2e.rs Cargo.toml src/main.rs
git commit -m "test(usage): end-to-end integration test against fixtures"
```

---

### Task 27: README + pricing-refresh script

**Files:**
- Modify: `README.md`
- Create: `scripts/refresh-pricing.sh`

- [ ] **Step 1: Add a usage section to the README**

In `README.md`, after the `### oronzo search <query>` section, insert:

````markdown
### `oronzo usage [report]`

Aggregate token usage and USD cost across all local Claude Code sessions. Defaults to a daily report.

```bash
oronzo usage                              # daily summary
oronzo usage monthly --since 20260101
oronzo usage session --project frontend
oronzo usage blocks --active              # current 5-hour window
oronzo usage daily --breakdown --json     # per-model rows as JSON
```

| Flag | Meaning |
|---|---|
| `--since YYYYMMDD` / `--until YYYYMMDD` | inclusive date bounds (also accepts `YYYY-MM-DD`) |
| `--project <substr>` | filter by cwd substring |
| `--breakdown` | per-model rows inside each bucket |
| `--instances` | (daily/weekly/monthly) split by project |
| `--timezone <IANA>` | e.g. `America/Los_Angeles`; defaults to system local |
| `--json` | machine-readable output |
| `--offline` | use bundled pricing snapshot; skip network |
| `--debug` | print parse/dedup stats to stderr |

Blocks-only: `--active` (open window only), `--recent N` (last N closed; default 10).

Pricing data comes from [LiteLLM](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json) and is refreshed at most once per day, cached at `~/.cache/oronzo/pricing.json`. A bundled snapshot ships with the binary as a fallback.
````

- [ ] **Step 2: Add the maintenance script**

Create `scripts/refresh-pricing.sh`:

```bash
#!/usr/bin/env bash
# Refresh the bundled pricing snapshot from LiteLLM.
# Run this manually when Anthropic publishes new pricing.
set -euo pipefail

URL="https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
DEST="$(git rev-parse --show-toplevel)/src/usage/pricing.json"

echo "Fetching $URL"
curl -fsSL "$URL" | jq 'with_entries(select(.key | test("^claude-")))' > "$DEST"
echo "Updated $DEST ($(wc -c < "$DEST") bytes)"
```

Mark it executable:

```bash
chmod +x scripts/refresh-pricing.sh
```

- [ ] **Step 3: Commit**

```bash
git add README.md scripts/refresh-pricing.sh
git commit -m "docs(usage): document the usage command and add pricing-refresh script"
```

---

## Self-review notes

- **Spec coverage:** every spec section is hit — command surface (T11/T12/T24/T25), discovery refactor (T2), help rename (T3), parse (T7/T8), dedup (T9), pricing (T4/T5/T6), filter (T14), aggregations (T15–T19), cache (T10/T23), render (T20/T21), breakdown/instances (T22), wiring (T24), help (T25), tests/fixtures (T7/T8/T9 fixtures + T26), README + pricing-refresh script (T27).
- **Type consistency:** `UsageRow`, `Pricing::compute_cost(model, input, output, cache_creation, cache_read)`, `ActiveTz::date_of/ymd_label/iso_week_label/ym_label/start_of_day_utc`, `Bucket`, `ReportData`, `ReportKind` all referenced with the same shapes across tasks.
- **Implementation notes:**
  - `blocks_filter` (Task 19) keeps the last N closed windows (by sorted `first` timestamp) plus any active window.
  - `--breakdown` (Task 12 validation, Task 22 implementation) is restricted to `daily`/`weekly`/`monthly` to keep the breakdown algorithm simple. Supporting it for `session` and `blocks` would require per-bucket re-aggregation and is deferred — the parser now rejects the combination with a clear error.
