# `oronzo mdexport` — design

**Status:** approved (brainstorming)
**Date:** 2026-05-15
**Owner:** enzol

## Goal

Add an `mdexport` subcommand to oronzo that dumps a single Claude Code session as Markdown on stdout. Useful for sharing, archiving, and feeding sessions back into other tools.

Inspirations: [simonw/claude-code-transcripts](https://github.com/simonw/claude-code-transcripts) (block taxonomy and per-tool special cases) and [withLinda/claude-JSONL-browser](https://github.com/withLinda/claude-JSONL-browser).

## Command surface

```
oronzo mdexport [query] [flags]
```

### Selection model

| Input | 0 matches | 1 match | 2+ matches |
|---|---|---|---|
| _(no query)_ | n/a (picker lists 30 most recent across all projects) | n/a | n/a |
| UUID-prefix (≥8 chars, only `[0-9a-f-]`, no spaces) | error `no session matches prefix '...'`, exit 1 | direct export, no picker | picker showing only the matching sessions |
| Word query | error `no results for '...'`, exit 1 | picker with 1 entry (always confirms before dumping) | picker with up to top 30 BM25-ranked sessions |

Word query never falls through to direct export; UUID-prefix never falls through to BM25. The two paths are disjoint.

Picker UI writes to stderr; the rendered Markdown is the only thing on stdout. This makes `oronzo mdexport > out.md` work cleanly even when a picker is shown.

### Picker UI

Each line in the picker has the format:

```
  i. <first-msg>  (<project>, <YYYY-MM-DD HH:MM>)
```

- `i` is 1-based and right-padded to width 2.
- `<first-msg>` is the first non-meta user `Text` block, with newlines replaced by spaces and truncated to 90 chars (ellipsis-suffixed if cut).
- `<project>` is the raw `cwd` of the first entry with one, abbreviated by replacing the user's home directory with `~`.
- `<YYYY-MM-DD HH:MM>` is the file's mtime in the system local timezone.
- When the query is a BM25 word query, the line additionally starts with `[score]` (4-decimal) before the index, matching `oronzo search`'s look-and-feel.

After printing the table, prompt `Select number (Enter to cancel): ` on stderr. Empty input cancels (exit 0, no output). Invalid number → error and exit 1.

### Flags

| Flag | Effect |
|---|---|
| `--no-tools` | drop `tool_use` and `tool_result` blocks |
| `--no-thinking` | drop `thinking` blocks |
| `--no-sidechains` | drop entries with `isSidechain: true` |
| `--no-images` | replace image blocks with a `_(image omitted: <media_type>, <N> bytes)_` placeholder |
| `-h`, `--help` | usage text |

Default behavior includes all four block classes. The flags are subtractive only — there is no `--tools-only` mode.

## Architecture

Single new module: `src/mdexport.rs` (~400 LOC). Internal layout:

| Function | Purpose |
|---|---|
| `pub fn run(args: &[String])` | Entry point. Parses args, dispatches to picker/direct path, writes Markdown to stdout. |
| `fn parse_args(args) -> Result<Args, String>` | Hand-rolled flag parser, same style as `mv.rs` and `usage::args`. |
| `fn select_session(args) -> Option<SessionFile>` | Chooses the session to export: direct UUID lookup, BM25 picker, or recent-sessions picker. Returns `None` if the user cancels at the picker. |
| `fn list_recent_sessions(claude_dir, limit) -> Vec<SessionInfo>` | Discovers, sorts by mtime, attaches a one-line summary. |
| `fn rank_with_query(claude_dir, query) -> Vec<SessionInfo>` | BM25 over the existing search-cache text (reused from `cmd_search`). |
| `fn resolve_uuid_prefix(claude_dir, prefix) -> Vec<SessionFile>` | Returns 0, 1, or many session files whose stem starts with `prefix`. |
| `fn parse_session(path) -> Session` | Walks one JSONL into a `Session { meta, entries }` value. |
| `fn render(session, args) -> String` | Returns the full Markdown string. |

Wiring:
- `main.rs::main()` gets a new arm: `"mdexport" => mdexport::run(&args[2..])`.
- `main.rs::help_text()` gains a new line in the `Commands:` block: `mdexport [query]   Export a session as markdown`.
- Place the `mdexport [query]` line between `search` and `usage` (so the read-only "look at sessions" commands cluster together).

### Reuse and duplication

- Reuse `sessions::discover()` and `sessions::claude_dir()` directly.
- BM25 scoring: a copy of the ~20-line BM25 helper from `src/main.rs`. Refactoring `main.rs::cmd_search` to expose BM25 is out of scope; a small duplication is preferable to a side-quest. (Tracked as a follow-up if either site changes.)
- Search-cache reuse: `mdexport` reads but does NOT write to `~/.cache/claude-search/index.json`. If the cache is empty/stale, BM25 falls back to direct parsing of each JSONL. (Avoids cross-feature cache-ownership confusion.)

### Internal types

```rust
struct Args {
    query: Option<String>,
    tools: bool,
    thinking: bool,
    sidechains: bool,
    images: bool,           // all four default true
}

struct SessionInfo {
    path: PathBuf,
    id: String,
    project: String,        // raw cwd or fallback to parent dir name
    first_msg: String,
    mtime: f64,
    score: Option<f64>,     // Some only when a BM25 query was given
}

struct Session {
    meta: SessionMeta,
    entries: Vec<Entry>,
}

struct SessionMeta {
    id: String,
    project: String,        // cwd from the first entry that has one
    git_branch: Option<String>,
    started: chrono::DateTime<chrono::Utc>,
    ended: chrono::DateTime<chrono::Utc>,
    message_count: usize,
    models: Vec<String>,    // distinct, in first-seen order
}

struct Entry {
    timestamp: chrono::DateTime<chrono::Utc>,
    role: Role,             // User | Assistant
    is_sidechain: bool,
    is_meta: bool,
    is_compact_summary: bool,
    blocks: Vec<Block>,
}

enum Role { User, Assistant }

enum Block {
    Text(String),
    Thinking(String),
    ToolUse { name: String, input: serde_json::Value, id: String },
    ToolResult { content: ToolResultContent, is_error: bool },
    Image { media_type: String, data: String },
}

enum ToolResultContent {
    Text(String),
    Blocks(Vec<Block>),     // when tool result is mixed text + images
}
```

`is_meta` is honored at the entry level: meta entries (system warmup, etc.) are skipped from output. Their `tool_use`/`tool_result` chains are skipped too.

## Markdown format

### Header

Emitted once at the top of every export:

```markdown
# Session <session-id>

| Project | <cwd> |
|---|---|
| Started | <RFC3339> |
| Ended | <RFC3339> |
| Messages | <non-meta entry count> |
| Models | <comma-separated unique models, in first-seen order> |
| Git branch | <branch> |

---

```

The `Git branch` row is omitted if no entry carries a `gitBranch` field.

### Per entry

Each non-meta entry becomes a level-2 heading:

```markdown
## <Role> · <YYYY-MM-DD HH:MM:SS UTC>

<blocks…>

```

`<Role>` is `User` or `Assistant`.

### Block rendering

| Block | Markdown |
|---|---|
| `Text` | emitted as-is; assumed valid Markdown. |
| `Thinking` | `<details><summary>💭 Thinking</summary>\n\n<text>\n\n</details>` |
| `ToolUse` Bash | optional italic `_<description>_` line, then ` ```bash\n<command>\n``` ` |
| `ToolUse` Edit | `**Edit: \`<path>\`**` header, then a ```diff block with `- old_string`/`+ new_string`. If `replace_all`, append `_(replace_all)_`. |
| `ToolUse` Write | `**Write: \`<path>\`**` header, then a ```` ```<lang>\n<content>\n``` ```` block. `<lang>` inferred from extension (`rs`→`rust`, `py`→`python`, `js`/`ts`→`typescript`/`javascript`, etc.; unknown extensions fall back to `text`). |
| `ToolUse` Read | `**Read: \`<path>\`**` header. If `offset` or `limit` set, append `_(offset N, limit M)_`. No body — content arrives in the next tool result. |
| `ToolUse` TodoWrite | markdown checklist of `todos`: `- [ ]` for pending, `- [x]` for completed, `- [ ] 🚧 <text>` for in_progress. |
| `ToolUse` other | `**Tool: <name>**` header + ```json block of the full `input`. |
| `ToolResult` | string content → ```text block; block-array content → each text block as ```text and each image per row below. `is_error: true` prepends `**❌ Tool error:**`. |
| `Image` | `![image](data:<media_type>;base64,<data>)` unless `--no-images`. |

### Sidechain spans

When the entry stream transitions from non-sidechain to sidechain, emit:

```markdown
### 🤖 Subagent task

```

When it transitions back, emit:

```markdown
### ← Resuming main thread

```

Multiple subagent spans get sequential headings. `--no-sidechains` removes the entries entirely (and their bracketing headings).

### Language inference for Write blocks

Lookup table (extension → fence language):

| ext | lang | ext | lang |
|---|---|---|---|
| `rs` | `rust` | `py` | `python` |
| `ts` / `tsx` | `typescript` | `js` / `jsx` | `javascript` |
| `go` | `go` | `rb` | `ruby` |
| `java` | `java` | `kt` | `kotlin` |
| `c` / `h` | `c` | `cpp` / `hpp` / `cc` / `hh` | `cpp` |
| `sh` / `bash` | `bash` | `zsh` | `zsh` |
| `json` | `json` | `yaml` / `yml` | `yaml` |
| `toml` | `toml` | `md` / `markdown` | `markdown` |
| `html` | `html` | `css` | `css` |
| `sql` | `sql` | `xml` | `xml` |

Unknown extensions, no extension, or paths ending in `/` → `text`.

## Error handling

| Condition | Behavior |
|---|---|
| `~/.claude/projects/` missing | `error: Claude sessions directory not found at <path>`, exit 1 |
| UUID-prefix matches nothing | `error: no session matches prefix '<prefix>'`, exit 1 |
| Word query matches nothing | `error: no results for '<query>'`, exit 1 |
| User cancels picker (empty Enter) | exit 0, no output |
| Picker selection out of range | `error: invalid selection`, exit 1 |
| JSONL line fails to parse | skipped silently (consistent with existing parsers in this codebase) |
| Selected session file unreadable | `error: cannot read <path>: <io error>`, exit 1 |

## Testing

Fixtures under `tests/fixtures/mdexport/`:

- `proj_a/sess_with_tools.jsonl` — one user message, one assistant text, one Bash tool_use, one tool_result, one TodoWrite tool_use.
- `proj_a/sess_with_thinking.jsonl` — thinking block + text block.
- `proj_a/sess_with_sidechain.jsonl` — main thread → sidechain entries → main thread.
- `proj_a/sess_with_image.jsonl` — assistant message containing an image block (small synthetic base64).
- `proj_a/sess_with_edit.jsonl` — Write + Edit tool calls.
- `proj_a/sess_meta_only.jsonl` — entries all `isMeta: true`; should produce a header-only export.

Unit tests in `src/mdexport.rs`:

- `parse_args` covers each flag and the unknown-flag error path.
- `resolve_uuid_prefix` returns 0 / 1 / 2 matches against fixture files.
- Block-rendering tests: one per block kind (text, thinking, Bash/Edit/Write/Read/TodoWrite/other, tool_result text + blocks, image, image-omitted).
- A round-trip test on `sess_with_tools.jsonl` that asserts a snapshot of the rendered Markdown.
- Sidechain bracketing test on `sess_with_sidechain.jsonl`.

Integration test (in `tests/usage_e2e.rs` or a new `tests/mdexport_e2e.rs`):

- Invokes the public API end-to-end on the fixture tree and asserts substrings of the rendered output.

## Non-goals

- Multi-session batch export (export one at a time).
- HTML or PDF output (Markdown only).
- Interactive in-place editing of the export before save.
- Following or rewriting links inside session text.
- Including raw token-usage numbers in the export (`oronzo usage` covers that).
- Refactoring `main.rs::cmd_search` to share BM25 internals.
- Writing to the search cache from `mdexport`.
