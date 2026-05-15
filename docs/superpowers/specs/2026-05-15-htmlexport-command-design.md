# `oronzo htmlexport` — design

**Status:** approved (brainstorming)
**Date:** 2026-05-15
**Owner:** enzol

## Goal

Add an `htmlexport` subcommand that emits a single self-contained HTML document for one Claude Code session. Visually parallels [`mdexport`](2026-05-15-mdexport-command-design.md) but uses a chat-bubble layout, an embedded stylesheet, inline base64 images, and `pulldown-cmark`-rendered markdown for prose. No external resources, no JavaScript.

## Command surface

```
oronzo htmlexport [query] [flags]
```

Selection model is identical to `mdexport`:

| Input | 0 matches | 1 match | 2+ matches |
|---|---|---|---|
| _(no query)_ | n/a (picker lists 30 most recent) | n/a | n/a |
| UUID-prefix (≥8 chars, only `[0-9a-f-]`, no spaces) | error `no session matches prefix '...'`, exit 1 | direct export | picker showing only matching sessions |
| Word query | error `no results for '...'`, exit 1 | picker with 1 entry | picker with top 30 BM25-ranked |

Picker UI on stderr; HTML on stdout. Empty picker input cancels (exit 0, no stdout). Out-of-range picker input → `invalid selection`, exit 1. UUID-prefix never falls through to BM25; word query never falls through to direct export.

### Flags

| Flag | Effect |
|---|---|
| `--no-tools` | drop `tool_use`/`tool_result` blocks |
| `--no-thinking` | drop `thinking` blocks |
| `--no-sidechains` | drop subagent (sidechain) entries |
| `--no-images` | replace image blocks with a `<div class="image-omitted">` placeholder |
| `-h`, `--help` | usage text on stderr, exit 0 |

Defaults include all four block classes.

## Architecture

Single new file `src/htmlexport.rs` (~500 LOC). All selection and parsing infrastructure is reused from `mdexport`; only the renderer is new.

| Function | Purpose |
|---|---|
| `pub fn run(args: &[String])` | Entry point. Parses args, picks session via `mdexport::select_session`, parses via `mdexport::parse_session`, prints HTML. |
| `fn parse_args(args) -> Result<Args, String>` | Hand-rolled flag parser, same shape as `mdexport::parse_args`. Returns `Err("__help__")` for `-h`/`--help`. |
| `pub fn render(session, args) -> String` | Builds the full HTML document. |
| `fn render_block(block, args) -> String` | Per-block rendering. |
| `fn render_tool_use(name, input) -> String` | Per-tool special cases (Bash/Edit/Write/Read/TodoWrite + JSON fallback). |
| `fn render_tool_result(content, is_error, args) -> String` | tool_result rendering. |
| `fn render_markdown(s) -> String` | `pulldown-cmark` (CommonMark + tables) → HTML. |
| `fn html_escape(s) -> String` | Hand-rolled escape for `& < > " '`. No new dep beyond pulldown-cmark. |
| `pub fn help() -> String` | Usage text. |

### Reused from `mdexport`

Called directly via `crate::mdexport::*`:

- Types reused: `Session`, `SessionMeta`, `SessionInfo`, `Entry`, `Role`, `Block`, `ToolResultContent`.
- Functions reused: `select_session`, `parse_session`, `list_recent_sessions`, `rank_with_query`, `resolve_uuid_prefix`, `looks_like_uuid_prefix`, `first_user_text`, `format_picker_line`, `prompt_selection`, `parse_selection`, `pick_from`, `lang_for_path` (and transitively, `sessions::discover`).

`htmlexport` defines its own `Args` struct with the same field set as `mdexport::Args`. The duplication is deliberate — sharing the type would couple two otherwise-independent commands' flag evolution. The local struct stays ~15 lines.

**`mdexport::select_session` takes `&mdexport::Args`**, so when `htmlexport::run` invokes selection it must construct a temporary `mdexport::Args` whose `query`, `tools`, `thinking`, `sidechains`, `images` fields mirror the local args. This is mechanical (a 1:1 field copy) and lives entirely inside `htmlexport::run`. The render path uses the local `htmlexport::Args` directly.

### Wiring

- `main.rs` gets `pub mod htmlexport;` near the other `pub mod` declarations.
- New dispatch arm: `"htmlexport" => htmlexport::run(&args[2..]),` right after the `"mdexport"` arm.
- `main.rs::help_text()` gains a `htmlexport [query]   Export a session as HTML` line between `mdexport` and `usage`.

### New dependency

`pulldown-cmark = "0.13"` with default features only. Adds ~150 KB to the binary. Used solely for `render_markdown`.

## CSS / visual design

Embedded via `include_str!("htmlexport.css")` in a single inline `<style>` block in the document head. The file `src/htmlexport.css` is plain CSS, edited independently of Rust.

### Document structure

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Session <id></title>
  <style>/* embedded CSS */</style>
</head>
<body>
  <header class="meta">
    <h1>Session <id></h1>
    <dl>
      <dt>Project</dt><dd><cwd></dd>
      <dt>Started</dt><dd><RFC3339></dd>
      <dt>Ended</dt><dd><RFC3339></dd>
      <dt>Messages</dt><dd><non-meta count></dd>
      <dt>Models</dt><dd><comma-separated></dd>
      <dt>Git branch</dt><dd><branch></dd>  <!-- omitted if absent -->
    </dl>
  </header>

  <main class="conversation">
    <!-- one <section> per non-meta entry -->
    <!-- sidechain banners between turns -->
  </main>

  <footer class="footer">
    Generated by oronzo <CARGO_PKG_VERSION> · <chrono::Utc::now().to_rfc3339()>
  </footer>
</body>
</html>
```

### Per-entry rendering

```html
<section class="turn turn-user">
  <header class="turn-head">
    <span class="role">User</span>
    <time>2026-05-07T10:00:00Z</time>
  </header>
  <div class="turn-body">
    <!-- blocks rendered here -->
  </div>
</section>
```

`turn-assistant` for the alternate class. User bubbles right-aligned with a blue tint; assistant bubbles left-aligned, neutral background. Both max-width 60% of the conversation column on desktop, full-width below 600px.

### Per-block HTML

| Block | HTML |
|---|---|
| `Text` | `pulldown-cmark` output (paragraphs, lists, tables, inline code, fenced code blocks). All text in the input gets escaped by the parser. |
| `Thinking` | `<details class="thinking"><summary>💭 Thinking</summary>{rendered markdown}</details>` (collapsed by default). Suppressed entirely if `--no-thinking`. |
| `ToolUse` Bash | `<div class="tool tool-bash"><div class="desc"><em>{escape(description)}</em></div><pre class="code language-bash">{escape(command)}</pre></div>` — description div omitted if empty. |
| `ToolUse` Edit | `<div class="tool tool-edit"><div class="path"><code>{escape(path)}</code><span class="replace-all">replace_all</span></div><pre class="diff">{lines with `<span class="del">` / `<span class="add">` wrappers}</pre></div>` — `<span class="replace-all">` omitted unless flagged. |
| `ToolUse` Write | `<div class="tool tool-write"><div class="path"><code>{escape(path)}</code></div><pre class="code language-{lang}">{escape(content)}</pre></div>` — language inferred via shared `mdexport::lang_for_path`. |
| `ToolUse` Read | `<div class="tool tool-read"><div class="path"><code>{escape(path)}</code><span class="range">{offset/limit if present}</span></div></div>` |
| `ToolUse` TodoWrite | `<div class="tool tool-todo"><ul class="todos"><li class="todo-pending">…</li><li class="todo-in-progress">…</li><li class="todo-done">…</li></ul></div>` |
| `ToolUse` other | `<div class="tool tool-other"><div class="name"><strong>Tool: {escape(name)}</strong></div><pre class="json">{escape(pretty_json)}</pre></div>` |
| `ToolResult` string | `<div class="result"><pre>{escape(content)}</pre></div>`; `<div class="result error">` and ❌ prefix when `is_error`. |
| `ToolResult` blocks | `<div class="result">{recurse into render_block per sub-block}</div>` — honors `--no-images` inside. |
| `Image` | `<figure class="image"><img alt="image" src="data:{media_type};base64,{data}"></figure>` |
| `Image` (omitted) | `<div class="image-omitted">image omitted: {media_type}, {decoded_bytes} bytes</div>` |

### Sidechain spans

Open: `<div class="subagent-banner">🤖 Subagent task</div>`
Close: `<div class="subagent-banner end">← Resuming main thread</div>`

Banners span the full conversation column (not bubbled). Dashed border. Trailing-span flush handled identically to `mdexport::render`.

### Styling rules (CSS file targets these)

- Body: max-width 820px, centered, padding 1.5rem. Background `#f5f5f7` light / `#1a1a1d` dark. System sans-serif stack.
- Meta header card: white/dark-gray card, rounded 12px, padding 1.25rem, subtle shadow, definition-list with bold `<dt>` left, `<dd>` right.
- Turn bubbles: rounded 16px, padding 1rem, max-width 60%, top margin 1.25rem. User bubble background `#dbeafe` / `#1e3a5f`, assistant bubble white / `#2a2a2e`.
- Role pill: small-caps, muted gray, 0.8rem.
- Code blocks (`pre`): background `#f3f4f6` / `#2e2e33`, padding 0.75rem, border-radius 8px, overflow-x: auto, 0.9rem monospace (system mono stack).
- Inline `<code>`: same background, 2px padding, 0.9em.
- Tool cards: 4px left border keyed by tool class — bash `#10b981`, edit `#f59e0b`, write `#8b5cf6`, read `#9ca3af`, todo `#eab308`, other `#9ca3af`. Background slightly tinted gray. Padding 0.75rem. Margin 0.5rem 0.
- Diff: `.del` light-red text background, `.add` light-green text background. Match `git diff` color conventions.
- Tool result: `<div class="result">` with light-gray background and `<pre>` inside. `result.error` adds light-red background and a red ❌ glyph row at top.
- TodoWrite list: `<ul.todos>` no bullets; each `<li>` prefixes a UTF-8 checkbox based on status class (`☐` for pending, `▶` for in_progress, `☑` for done). Status word in small-caps muted text.
- Sidechain banner: dashed 1px border, italic centered text, no bubble background, 0.5rem padding.
- Image: max-width 100%, rounded 8px, displayed as figure with no caption.
- Footer: centered, muted small text, 1rem top margin.
- Mobile (`max-width: 600px`): turn bubble grows to 100% width, page padding shrinks to 0.75rem, meta dl stacks (dt full-width above dd).
- Dark mode via `@media (prefers-color-scheme: dark)`: invert palette, keep accent colors (left borders, diff colors) unchanged.

The compiled CSS file ends up roughly 200–250 lines. No external fonts. No JS.

## Error handling

Identical to `mdexport`:

| Condition | Behavior |
|---|---|
| `~/.claude/projects/` missing | `error: Claude sessions directory not found at <path>`, exit 1 |
| UUID-prefix matches nothing | `error: no session matches prefix '<prefix>'`, exit 1 |
| Word query matches nothing | `error: no results for '<query>'`, exit 1 |
| User cancels picker (empty Enter) | exit 0, no output |
| Picker selection out of range or non-numeric | `error: invalid selection`, exit 1 |
| Selected file unreadable | `error: cannot read <path>: <io error>`, exit 1 |
| JSONL line fails to parse | skipped silently (same as `mdexport`) |

## Testing

Reuses the fixture tree at `tests/fixtures/mdexport/proj_a/` — no new fixtures required.

Unit tests in `src/htmlexport.rs`:

- `parse_args` covers each flag and the unknown-flag error path (mirrors `mdexport` parser tests).
- `html_escape` covers all five entities.
- `render_markdown` covers a basic markdown round-trip (`**bold**` → `<strong>bold</strong>`).
- Block-rendering tests:
  - `render_text_block_renders_markdown` — `*emph*` → `<em>emph</em>`.
  - `render_thinking_block_uses_details`.
  - `render_thinking_skipped_when_disabled`.
  - `render_image_emits_img_tag`.
  - `render_image_omitted_emits_placeholder`.
  - `render_bash_includes_command_and_description`.
  - `render_edit_emits_diff_with_del_and_add_spans`.
  - `render_write_uses_inferred_language`.
  - `render_read_with_offset_limit`.
  - `render_todowrite_emits_classed_list_items`.
  - `render_other_tool_falls_back_to_json_pre`.
  - `render_tool_result_text`.
  - `render_tool_result_error_uses_error_class`.
  - `render_tool_blocks_skipped_when_disabled`.
- Session-rendering tests:
  - `render_emits_doctype_and_meta_header` — verifies the head, viewport meta, embedded `<style>` block, and meta card content.
  - `render_skips_meta_entries`.
  - `render_brackets_sidechain_spans`.
  - `render_drops_sidechain_entries_when_disabled`.
  - `render_closes_trailing_sidechain_span`.

Integration test in `tests/htmlexport_e2e.rs` (parallel to `tests/mdexport_e2e.rs`):

- `export_sess_with_tools_to_html` — renders the existing tools fixture; asserts substrings: DOCTYPE, `<title>Session 11111111`, `class="turn turn-user"`, `class="turn turn-assistant"`, `class="tool tool-bash"`, `language-bash`, `class="tool tool-todo"`, `class="todo-in-progress"`, `class="todo-done"`.
- `export_meta_only_yields_empty_conversation` — header card present, but `<main class="conversation">` is empty.
- `flag_strips_tools_and_thinking` — assert no `class="tool"` and no `class="thinking"` substrings.
- `output_is_self_contained` — assert the output contains no `<script>` tag, no `<link rel="stylesheet"`, and no `src="http`/`src="https`/`src="//` (only `data:` URLs allowed).

## README

Add a new section to `README.md` right after the `mdexport` section:

````markdown
### `oronzo htmlexport [query]`

Export a single Claude Code session to a self-contained HTML document on stdout. Same selection model as `mdexport`; same toggle flags.

```bash
oronzo htmlexport > out.html
oronzo htmlexport "fix auth bug" > bug.html
oronzo htmlexport 11111111 > session.html
oronzo htmlexport --no-tools --no-thinking
```

CSS is embedded inline; images go in as base64 `data:` URLs; no external resources, no JavaScript. Light + dark theme via `prefers-color-scheme`. Mobile-friendly down to 320px.

| Flag | Effect |
|---|---|
| `--no-tools` | drop `tool_use`/`tool_result` blocks |
| `--no-thinking` | drop `thinking` blocks |
| `--no-sidechains` | drop subagent entries |
| `--no-images` | replace image blocks with a placeholder |
````

## Non-goals

- Multi-session batch export.
- Syntax highlighting in code blocks (plain monospace; can layer `syntect` later).
- Light/dark toggle button (relies on system preference).
- External font loading.
- Anything other than CommonMark + tables in markdown.
- Refactoring `mdexport` to expose its `Args` shape; the duplication is small and intentional.
