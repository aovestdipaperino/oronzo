# `oronzo htmlexport` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `htmlexport` subcommand that emits a single self-contained HTML document for one Claude Code session, with embedded CSS, base64 images, `pulldown-cmark`-rendered markdown, and a chat-bubble visual layout. Selection model identical to `mdexport`.

**Architecture:** Two new files — `src/htmlexport.rs` (~500 LOC) for the renderer and dispatch, and `src/htmlexport.css` (~250 lines) embedded via `include_str!`. Reuses every selection and parsing helper from `mdexport`; only the renderer is new.

**Tech Stack:** Rust 2024. New dep: `pulldown-cmark = "0.13"` (default features only). Existing deps cover everything else.

**Spec:** [`docs/superpowers/specs/2026-05-15-htmlexport-command-design.md`](../specs/2026-05-15-htmlexport-command-design.md)

---

## File Structure

| File | Responsibility |
|---|---|
| `src/htmlexport.rs` (new) | `Args`, `parse_args`, `render`, `render_block`, `render_tool_use`, `render_tool_result`, `render_markdown`, `html_escape`, `run`, `help`. |
| `src/htmlexport.css` (new) | All visual styling — chat bubbles, tool cards, code blocks, light/dark mode, mobile breakpoints. Embedded via `include_str!`. |
| `src/main.rs` (modified) | One new `pub mod htmlexport;` declaration; one new dispatch arm; one new line in `help_text()`. |
| `tests/htmlexport_e2e.rs` (new) | Integration tests via the public lib API. |
| `Cargo.toml` (modified) | Add `pulldown-cmark = "0.13"`. |
| `README.md` (modified) | New section after `mdexport`. |

The fixture tree at `tests/fixtures/mdexport/proj_a/` is reused as-is; no new JSONL files required.

---

## Type contracts (referenced by multiple tasks)

```rust
// src/htmlexport.rs

#[derive(Debug, PartialEq)]
pub struct Args {
    pub query: Option<String>,
    pub tools: bool,
    pub thinking: bool,
    pub sidechains: bool,
    pub images: bool,
}
```

Block-level types (`Session`, `Entry`, `Role`, `Block`, `ToolResultContent`, `SessionMeta`) come from `crate::mdexport`. They are imported, not redefined.

`mdexport::Args` has the same field shape, so `htmlexport::run` translates between the two when calling `mdexport::select_session`:

```rust
let mdex_args = crate::mdexport::Args {
    query: parsed.query.clone(),
    tools: parsed.tools,
    thinking: parsed.thinking,
    sidechains: parsed.sidechains,
    images: parsed.images,
};
```

---

### Task 1: Add `pulldown-cmark` dep and scaffold `htmlexport.rs`

**Files:**
- Modify: `Cargo.toml`
- Create: `src/htmlexport.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add the dep**

In `Cargo.toml`, under `[dependencies]`, add:

```toml
pulldown-cmark = { version = "0.13", default-features = false, features = ["html"] }
```

- [ ] **Step 2: Create the stub module**

Create `src/htmlexport.rs`:

```rust
pub fn run(_args: &[String]) {
    eprintln!("htmlexport: not yet implemented");
    std::process::exit(2);
}
```

- [ ] **Step 3: Register the module**

In `src/main.rs`, add `pub mod htmlexport;` near the other `pub mod` declarations (after `pub mod mdexport;`).

- [ ] **Step 4: Add the dispatch arm**

In `src/main.rs::main()`, find the `match args[1].as_str()` block. Right after the `"mdexport" => mdexport::run(&args[2..])` arm, add:

```rust
        "htmlexport" => htmlexport::run(&args[2..]),
```

- [ ] **Step 5: Verify**

Run: `cargo build`
Expected: clean build (downloads pulldown-cmark, then compiles).

Run: `cargo run --quiet -- htmlexport 2>&1 | head -1`
Expected: `htmlexport: not yet implemented`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/htmlexport.rs src/main.rs
git commit -m "feat(htmlexport): scaffold module, add pulldown-cmark, wire dispatch arm"
```

---

### Task 2: `Args` struct and `parse_args`

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Replace the stub with `Args` + parser + tests**

Replace `src/htmlexport.rs` with:

```rust
#[derive(Debug, PartialEq)]
pub struct Args {
    pub query: Option<String>,
    pub tools: bool,
    pub thinking: bool,
    pub sidechains: bool,
    pub images: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            query: None,
            tools: true,
            thinking: true,
            sidechains: true,
            images: true,
        }
    }
}

pub fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut query_parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--no-tools" => out.tools = false,
            "--no-thinking" => out.thinking = false,
            "--no-sidechains" => out.sidechains = false,
            "--no-images" => out.images = false,
            "-h" | "--help" => return Err("__help__".into()),
            s if s.starts_with("--") => return Err(format!("unknown flag: {s}")),
            _ => query_parts.push(a.clone()),
        }
        i += 1;
    }
    if !query_parts.is_empty() {
        out.query = Some(query_parts.join(" "));
    }
    Ok(out)
}

pub fn run(_args: &[String]) {
    eprintln!("htmlexport: not yet implemented");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_args_yields_defaults() {
        let a = parse_args(&argv(&[])).unwrap();
        assert_eq!(a, Args::default());
    }

    #[test]
    fn negation_flags_flip_each_class() {
        let a = parse_args(&argv(&["--no-tools", "--no-thinking", "--no-sidechains", "--no-images"])).unwrap();
        assert!(!a.tools);
        assert!(!a.thinking);
        assert!(!a.sidechains);
        assert!(!a.images);
    }

    #[test]
    fn positional_args_join_into_query() {
        let a = parse_args(&argv(&["fix", "auth", "bug"])).unwrap();
        assert_eq!(a.query.as_deref(), Some("fix auth bug"));
    }

    #[test]
    fn help_returns_sentinel_error() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap_err(), "__help__");
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(parse_args(&argv(&["--bogus"])).is_err());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test htmlexport::tests`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): args struct and flag parser"
```

---

### Task 3: HTML escape helper

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` block in `src/htmlexport.rs`:

```rust
    #[test]
    fn html_escape_handles_all_five_entities() {
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#39;s");
    }

    #[test]
    fn html_escape_passes_through_plain_text() {
        assert_eq!(html_escape("hello world"), "hello world");
        assert_eq!(html_escape(""), "");
    }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test htmlexport::tests::html_escape`
Expected: FAIL — function not defined.

- [ ] **Step 3: Implement `html_escape`**

Add to `src/htmlexport.rs` above the `#[cfg(test)]` block:

```rust
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test htmlexport::tests::html_escape`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): html_escape helper"
```

---

### Task 4: `render_markdown` via `pulldown-cmark`

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Add failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn render_markdown_renders_bold_and_italic() {
        let out = render_markdown("**bold** and *italic*");
        assert!(out.contains("<strong>bold</strong>"));
        assert!(out.contains("<em>italic</em>"));
    }

    #[test]
    fn render_markdown_renders_fenced_code_with_language_class() {
        let md = "```rust\nfn main() {}\n```";
        let out = render_markdown(md);
        assert!(out.contains("<code class=\"language-rust\">"));
        assert!(out.contains("fn main() {}"));
    }

    #[test]
    fn render_markdown_renders_tables() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let out = render_markdown(md);
        assert!(out.contains("<table>"));
        assert!(out.contains("<th>a</th>"));
        assert!(out.contains("<td>1</td>"));
    }

    #[test]
    fn render_markdown_escapes_raw_html_input() {
        // pulldown-cmark by default emits raw HTML tags as-is; we configure
        // it to skip them, so a literal "<script>" in markdown stays inert.
        let out = render_markdown("plain <script>x</script> text");
        assert!(!out.contains("<script>"));
    }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test htmlexport::tests::render_markdown`
Expected: 4 tests fail (function undefined).

- [ ] **Step 3: Implement `render_markdown`**

Add to `src/htmlexport.rs` above `#[cfg(test)]`:

```rust
use pulldown_cmark::{html, Options, Parser};

pub fn render_markdown(s: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(s, opts);
    // Strip raw HTML to avoid arbitrary script injection from session content.
    let safe = parser.filter_map(|event| match event {
        pulldown_cmark::Event::Html(_) | pulldown_cmark::Event::InlineHtml(_) => None,
        other => Some(other),
    });
    let mut out = String::new();
    html::push_html(&mut out, safe);
    out
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test htmlexport::tests::render_markdown`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): render_markdown via pulldown-cmark with raw-HTML stripped"
```

---

### Task 5: CSS file

**Files:**
- Create: `src/htmlexport.css`

- [ ] **Step 1: Create the stylesheet**

Create `src/htmlexport.css` with the full chat-bubble stylesheet:

```css
/* oronzo htmlexport — chat-bubble layout */

:root {
  --bg: #f5f5f7;
  --card-bg: #ffffff;
  --text: #1f2937;
  --muted: #6b7280;
  --border: #e5e7eb;
  --code-bg: #f3f4f6;
  --bubble-user: #dbeafe;
  --bubble-assistant: #ffffff;
  --accent-bash: #10b981;
  --accent-edit: #f59e0b;
  --accent-write: #8b5cf6;
  --accent-read: #9ca3af;
  --accent-todo: #eab308;
  --accent-other: #9ca3af;
  --diff-del-bg: #fee2e2;
  --diff-add-bg: #dcfce7;
  --error-bg: #fef2f2;
  --error-fg: #b91c1c;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #1a1a1d;
    --card-bg: #2a2a2e;
    --text: #e5e7eb;
    --muted: #9ca3af;
    --border: #3a3a3f;
    --code-bg: #2e2e33;
    --bubble-user: #1e3a5f;
    --bubble-assistant: #2a2a2e;
    --diff-del-bg: #4c1d1d;
    --diff-add-bg: #14532d;
    --error-bg: #3a1a1a;
    --error-fg: #fca5a5;
  }
}

* { box-sizing: border-box; }

body {
  margin: 0;
  padding: 1.5rem;
  max-width: 820px;
  margin-left: auto;
  margin-right: auto;
  font-family: -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  font-size: 16px;
  line-height: 1.55;
  color: var(--text);
  background: var(--bg);
}

h1 {
  margin: 0 0 1rem 0;
  font-size: 1.5rem;
}

a { color: #2563eb; }
@media (prefers-color-scheme: dark) { a { color: #60a5fa; } }

/* Meta header card */
header.meta {
  background: var(--card-bg);
  border-radius: 12px;
  padding: 1.25rem;
  margin-bottom: 1.5rem;
  box-shadow: 0 1px 3px rgba(0,0,0,0.05);
}
header.meta dl {
  margin: 0;
  display: grid;
  grid-template-columns: max-content 1fr;
  column-gap: 1rem;
  row-gap: 0.25rem;
}
header.meta dt {
  font-weight: 600;
  color: var(--muted);
}
header.meta dd {
  margin: 0;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.9rem;
  word-break: break-all;
}

/* Conversation */
main.conversation {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

section.turn {
  max-width: 60%;
  padding: 1rem;
  border-radius: 16px;
  background: var(--card-bg);
  box-shadow: 0 1px 2px rgba(0,0,0,0.04);
}
section.turn-user {
  background: var(--bubble-user);
  align-self: flex-end;
}
section.turn-assistant {
  background: var(--bubble-assistant);
  align-self: flex-start;
}

header.turn-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  margin-bottom: 0.5rem;
  font-size: 0.75rem;
  color: var(--muted);
}
header.turn-head .role {
  text-transform: uppercase;
  letter-spacing: 0.05em;
  font-weight: 700;
}
header.turn-head time {
  font-variant-numeric: tabular-nums;
}

/* Markdown content */
.turn-body > :first-child { margin-top: 0; }
.turn-body > :last-child { margin-bottom: 0; }
.turn-body p { margin: 0.5rem 0; }
.turn-body ul, .turn-body ol { margin: 0.5rem 0; padding-left: 1.5rem; }
.turn-body table {
  border-collapse: collapse;
  margin: 0.5rem 0;
}
.turn-body th, .turn-body td {
  border: 1px solid var(--border);
  padding: 0.25rem 0.5rem;
  text-align: left;
}
.turn-body pre {
  background: var(--code-bg);
  padding: 0.75rem;
  border-radius: 8px;
  overflow-x: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.875rem;
  line-height: 1.45;
  margin: 0.5rem 0;
}
.turn-body code {
  background: var(--code-bg);
  padding: 1px 4px;
  border-radius: 3px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.875em;
}
.turn-body pre code { background: none; padding: 0; }

/* Thinking */
details.thinking {
  margin: 0.5rem 0;
  border: 1px dashed var(--border);
  border-radius: 8px;
  padding: 0.5rem 0.75rem;
  background: var(--code-bg);
}
details.thinking summary {
  cursor: pointer;
  font-style: italic;
  color: var(--muted);
  user-select: none;
}
details.thinking[open] summary { margin-bottom: 0.5rem; }
details.thinking > :not(summary) { font-style: italic; color: var(--muted); }

/* Tool cards */
.tool {
  margin: 0.5rem 0;
  border-left: 4px solid var(--accent-other);
  background: var(--code-bg);
  padding: 0.6rem 0.75rem;
  border-radius: 0 8px 8px 0;
}
.tool-bash { border-left-color: var(--accent-bash); }
.tool-edit { border-left-color: var(--accent-edit); }
.tool-write { border-left-color: var(--accent-write); }
.tool-read { border-left-color: var(--accent-read); }
.tool-todo { border-left-color: var(--accent-todo); }

.tool .desc { color: var(--muted); margin-bottom: 0.4rem; }
.tool .path {
  font-size: 0.85rem;
  color: var(--muted);
  margin-bottom: 0.4rem;
}
.tool .path code {
  background: none;
  padding: 0;
  color: var(--text);
}
.tool .path .replace-all,
.tool .path .range {
  margin-left: 0.5rem;
  font-style: italic;
}
.tool pre {
  margin: 0;
  background: transparent;
  padding: 0;
}
.tool pre.diff .del {
  background: var(--diff-del-bg);
  display: block;
}
.tool pre.diff .add {
  background: var(--diff-add-bg);
  display: block;
}

/* Todos */
ul.todos { list-style: none; padding-left: 0; margin: 0; }
ul.todos li { padding: 0.15rem 0; }
ul.todos li::before { margin-right: 0.5rem; font-family: ui-monospace, monospace; }
ul.todos li.todo-pending::before { content: "☐"; }
ul.todos li.todo-in-progress::before { content: "▶"; color: var(--accent-todo); }
ul.todos li.todo-done::before { content: "☑"; color: var(--accent-bash); }
ul.todos li.todo-done { color: var(--muted); text-decoration: line-through; }

/* Tool result */
.result {
  margin-top: 0.4rem;
  background: var(--code-bg);
  border-radius: 6px;
  padding: 0.4rem 0.6rem;
}
.result pre {
  margin: 0;
  background: transparent;
  padding: 0;
  font-size: 0.85rem;
}
.result.error {
  background: var(--error-bg);
  color: var(--error-fg);
}
.result.error::before {
  content: "❌ Tool error";
  display: block;
  font-weight: 600;
  margin-bottom: 0.3rem;
}

/* Sidechain banner */
.subagent-banner {
  align-self: stretch;
  max-width: 100%;
  text-align: center;
  font-style: italic;
  color: var(--muted);
  border-top: 1px dashed var(--border);
  border-bottom: 1px dashed var(--border);
  padding: 0.5rem;
  margin: 0.25rem 0;
}

/* Images */
figure.image {
  margin: 0.5rem 0;
}
figure.image img {
  max-width: 100%;
  border-radius: 8px;
  display: block;
}
.image-omitted {
  font-style: italic;
  color: var(--muted);
  padding: 0.4rem 0.6rem;
  border: 1px dashed var(--border);
  border-radius: 6px;
}

/* Footer */
footer.footer {
  text-align: center;
  color: var(--muted);
  font-size: 0.8rem;
  margin-top: 2rem;
}

/* Mobile */
@media (max-width: 600px) {
  body { padding: 0.75rem; }
  section.turn { max-width: 100%; }
  header.meta dl {
    grid-template-columns: 1fr;
    row-gap: 0;
  }
  header.meta dt { margin-top: 0.5rem; }
}
```

- [ ] **Step 2: Verify it builds when included**

For now just confirm the file exists and has plain CSS. Run:

```bash
wc -l src/htmlexport.css
```

Expected: roughly 200–250 lines.

- [ ] **Step 3: Commit**

```bash
git add src/htmlexport.css
git commit -m "feat(htmlexport): embedded chat-bubble stylesheet"
```

---

### Task 6: `render_block` — Text, Thinking, Image

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Add failing tests**

Append inside `mod tests` in `src/htmlexport.rs`:

```rust
    use crate::mdexport::{Block, ToolResultContent};

    #[test]
    fn render_text_block_renders_markdown() {
        let out = render_block(&Block::Text("*emph*".into()), &Args::default());
        assert!(out.contains("<em>emph</em>"));
    }

    #[test]
    fn render_thinking_block_uses_details() {
        let out = render_block(&Block::Thinking("reasoning".into()), &Args::default());
        assert!(out.contains("<details class=\"thinking\""));
        assert!(out.contains("<summary>💭 Thinking</summary>"));
        assert!(out.contains("reasoning"));
    }

    #[test]
    fn render_thinking_skipped_when_disabled() {
        let mut a = Args::default();
        a.thinking = false;
        let out = render_block(&Block::Thinking("anything".into()), &a);
        assert!(out.is_empty());
    }

    #[test]
    fn render_image_emits_data_url_img() {
        let block = Block::Image { media_type: "image/png".into(), data: "iVBORw0KGgo=".into() };
        let out = render_block(&block, &Args::default());
        assert!(out.contains("<figure class=\"image\""));
        assert!(out.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""));
    }

    #[test]
    fn render_image_omitted_emits_placeholder() {
        let mut a = Args::default();
        a.images = false;
        let block = Block::Image { media_type: "image/png".into(), data: "iVBORw0KGgo=".into() };
        let out = render_block(&block, &a);
        assert!(out.contains("class=\"image-omitted\""));
        assert!(out.contains("image/png"));
        assert!(out.contains("8 bytes"));
    }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test htmlexport::tests::render_text_block_renders_markdown htmlexport::tests::render_thinking_block_uses_details htmlexport::tests::render_image_emits_data_url_img`
Expected: 3 tests fail — `render_block` undefined.

- [ ] **Step 3: Implement `render_block` with tool arms stubbed**

Add to `src/htmlexport.rs` above the `#[cfg(test)]` block:

```rust
use crate::mdexport::{Block, ToolResultContent};

fn decoded_bytes(b64: &str) -> usize {
    let trimmed = b64.trim_end_matches('=').len();
    trimmed * 3 / 4
}

pub fn render_block(block: &Block, args: &Args) -> String {
    match block {
        Block::Text(s) => render_markdown(s),
        Block::Thinking(s) => {
            if !args.thinking { return String::new(); }
            format!(
                "<details class=\"thinking\"><summary>💭 Thinking</summary>{}</details>\n",
                render_markdown(s)
            )
        }
        Block::Image { media_type, data } => {
            if !args.images {
                return format!(
                    "<div class=\"image-omitted\">image omitted: {}, {} bytes</div>\n",
                    html_escape(media_type),
                    decoded_bytes(data)
                );
            }
            format!(
                "<figure class=\"image\"><img alt=\"image\" src=\"data:{};base64,{}\"></figure>\n",
                html_escape(media_type),
                html_escape(data)
            )
        }
        Block::ToolUse { .. } | Block::ToolResult { .. } => {
            // Filled in by Task 7.
            String::new()
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test htmlexport::tests`
Expected: all 5 new tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): text/thinking/image block renderers"
```

---

### Task 7: `render_block` — Tool blocks (Bash/Edit/Write/Read/TodoWrite + JSON fallback + tool_result)

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Add failing tests**

Append inside `mod tests`:

```rust
    fn tool_use(name: &str, input: serde_json::Value) -> Block {
        Block::ToolUse { name: name.into(), input, id: "t-1".into() }
    }

    #[test]
    fn render_bash_includes_command_and_description() {
        let b = tool_use("Bash", serde_json::json!({"command":"ls -la","description":"list dir"}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"tool tool-bash\""));
        assert!(out.contains("<em>list dir</em>"));
        assert!(out.contains("<pre class=\"code language-bash\">ls -la</pre>"));
    }

    #[test]
    fn render_edit_emits_diff_with_del_and_add_spans() {
        let b = tool_use("Edit", serde_json::json!({
            "file_path":"/tmp/x.rs","old_string":"foo","new_string":"bar","replace_all":true
        }));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"tool tool-edit\""));
        assert!(out.contains("<code>/tmp/x.rs</code>"));
        assert!(out.contains("<span class=\"replace-all\">replace_all</span>"));
        assert!(out.contains("<span class=\"del\">- foo</span>"));
        assert!(out.contains("<span class=\"add\">+ bar</span>"));
    }

    #[test]
    fn render_write_uses_inferred_language() {
        let b = tool_use("Write", serde_json::json!({"file_path":"/tmp/a.rs","content":"fn main(){}"}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"tool tool-write\""));
        assert!(out.contains("<code>/tmp/a.rs</code>"));
        assert!(out.contains("<pre class=\"code language-rust\">fn main(){}</pre>"));
    }

    #[test]
    fn render_read_with_offset_limit() {
        let b = tool_use("Read", serde_json::json!({"file_path":"/tmp/a.rs","offset":10,"limit":50}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"tool tool-read\""));
        assert!(out.contains("<code>/tmp/a.rs</code>"));
        assert!(out.contains("<span class=\"range\">offset 10, limit 50</span>"));
    }

    #[test]
    fn render_todowrite_emits_classed_list_items() {
        let b = tool_use("TodoWrite", serde_json::json!({
            "todos":[
                {"content":"alpha","status":"pending"},
                {"content":"beta","status":"in_progress"},
                {"content":"gamma","status":"completed"}
            ]
        }));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"tool tool-todo\""));
        assert!(out.contains("<li class=\"todo-pending\">alpha</li>"));
        assert!(out.contains("<li class=\"todo-in-progress\">beta</li>"));
        assert!(out.contains("<li class=\"todo-done\">gamma</li>"));
    }

    #[test]
    fn render_other_tool_falls_back_to_json_pre() {
        let b = tool_use("Glob", serde_json::json!({"pattern":"*.rs"}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"tool tool-other\""));
        assert!(out.contains("<strong>Tool: Glob</strong>"));
        assert!(out.contains("<pre class=\"json\">"));
        assert!(out.contains("\"pattern\": \"*.rs\""));
    }

    #[test]
    fn render_tool_result_text() {
        let b = Block::ToolResult { content: ToolResultContent::Text("hello".into()), is_error: false };
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"result\""));
        assert!(out.contains("<pre>hello</pre>"));
    }

    #[test]
    fn render_tool_result_error_uses_error_class() {
        let b = Block::ToolResult { content: ToolResultContent::Text("boom".into()), is_error: true };
        let out = render_block(&b, &Args::default());
        assert!(out.contains("class=\"result error\""));
        assert!(out.contains("<pre>boom</pre>"));
    }

    #[test]
    fn render_tool_blocks_skipped_when_disabled() {
        let mut a = Args::default();
        a.tools = false;
        let bu = tool_use("Bash", serde_json::json!({"command":"ls"}));
        let br = Block::ToolResult { content: ToolResultContent::Text("x".into()), is_error: false };
        assert!(render_block(&bu, &a).is_empty());
        assert!(render_block(&br, &a).is_empty());
    }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test htmlexport::tests::render_bash_includes_command_and_description`
Expected: FAIL.

- [ ] **Step 3: Replace the `render_block` body and add helpers**

In `src/htmlexport.rs`, replace the `render_block` function body so the `Block::ToolUse` and `Block::ToolResult` arms dispatch to new helpers. Then add the helpers below `render_block`.

Replace `render_block` with:

```rust
pub fn render_block(block: &Block, args: &Args) -> String {
    match block {
        Block::Text(s) => render_markdown(s),
        Block::Thinking(s) => {
            if !args.thinking { return String::new(); }
            format!(
                "<details class=\"thinking\"><summary>💭 Thinking</summary>{}</details>\n",
                render_markdown(s)
            )
        }
        Block::Image { media_type, data } => {
            if !args.images {
                return format!(
                    "<div class=\"image-omitted\">image omitted: {}, {} bytes</div>\n",
                    html_escape(media_type),
                    decoded_bytes(data)
                );
            }
            format!(
                "<figure class=\"image\"><img alt=\"image\" src=\"data:{};base64,{}\"></figure>\n",
                html_escape(media_type),
                html_escape(data)
            )
        }
        Block::ToolUse { name, input, .. } => {
            if !args.tools { return String::new(); }
            render_tool_use(name, input)
        }
        Block::ToolResult { content, is_error } => {
            if !args.tools { return String::new(); }
            render_tool_result(content, *is_error, args)
        }
    }
}
```

Add these helpers below `render_block`:

```rust
fn render_tool_use(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let desc = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let mut out = String::from("<div class=\"tool tool-bash\">");
            if !desc.is_empty() {
                out.push_str(&format!("<div class=\"desc\"><em>{}</em></div>", html_escape(desc)));
            }
            out.push_str(&format!("<pre class=\"code language-bash\">{}</pre>", html_escape(cmd)));
            out.push_str("</div>\n");
            out
        }
        "Edit" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let old = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let new = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            let replace_all = input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut out = String::from("<div class=\"tool tool-edit\"><div class=\"path\"><code>");
            out.push_str(&html_escape(path));
            out.push_str("</code>");
            if replace_all {
                out.push_str("<span class=\"replace-all\">replace_all</span>");
            }
            out.push_str("</div><pre class=\"diff\">");
            for line in old.lines() {
                out.push_str(&format!("<span class=\"del\">- {}</span>", html_escape(line)));
            }
            for line in new.lines() {
                out.push_str(&format!("<span class=\"add\">+ {}</span>", html_escape(line)));
            }
            out.push_str("</pre></div>\n");
            out
        }
        "Write" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lang = crate::mdexport::lang_for_path(path);
            format!(
                "<div class=\"tool tool-write\"><div class=\"path\"><code>{}</code></div><pre class=\"code language-{}\">{}</pre></div>\n",
                html_escape(path),
                lang,
                html_escape(content)
            )
        }
        "Read" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let offset = input.get("offset").and_then(|v| v.as_u64());
            let limit = input.get("limit").and_then(|v| v.as_u64());
            let mut out = String::from("<div class=\"tool tool-read\"><div class=\"path\"><code>");
            out.push_str(&html_escape(path));
            out.push_str("</code>");
            if offset.is_some() || limit.is_some() {
                let o = offset.unwrap_or(0);
                let l = limit.unwrap_or(0);
                out.push_str(&format!("<span class=\"range\">offset {o}, limit {l}</span>"));
            }
            out.push_str("</div></div>\n");
            out
        }
        "TodoWrite" => {
            let mut out = String::from("<div class=\"tool tool-todo\"><ul class=\"todos\">");
            if let Some(todos) = input.get("todos").and_then(|v| v.as_array()) {
                for todo in todos {
                    let content = todo.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let status = todo.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let class = match status {
                        "completed" => "todo-done",
                        "in_progress" => "todo-in-progress",
                        _ => "todo-pending",
                    };
                    out.push_str(&format!("<li class=\"{}\">{}</li>", class, html_escape(content)));
                }
            }
            out.push_str("</ul></div>\n");
            out
        }
        other => {
            let json = serde_json::to_string_pretty(input).unwrap_or_else(|_| "{}".into());
            format!(
                "<div class=\"tool tool-other\"><div class=\"name\"><strong>Tool: {}</strong></div><pre class=\"json\">{}</pre></div>\n",
                html_escape(other),
                html_escape(&json)
            )
        }
    }
}

fn render_tool_result(content: &ToolResultContent, is_error: bool, args: &Args) -> String {
    let class = if is_error { "result error" } else { "result" };
    match content {
        ToolResultContent::Text(s) => {
            format!("<div class=\"{}\"><pre>{}</pre></div>\n", class, html_escape(s))
        }
        ToolResultContent::Blocks(blocks) => {
            let mut out = format!("<div class=\"{}\">", class);
            for b in blocks {
                out.push_str(&render_block(b, args));
            }
            out.push_str("</div>\n");
            out
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test htmlexport::tests`
Expected: 14 tests pass (5 from T6 + 9 new).

- [ ] **Step 5: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): per-tool renderers for Bash/Edit/Write/Read/TodoWrite + results"
```

---

### Task 8: `render` — full document with header, conversation, sidechain bracketing, footer

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Add failing tests**

Append inside `mod tests`:

```rust
    use crate::mdexport::parse_session;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn render_emits_doctype_and_meta_header() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_tools.jsonl")).unwrap();
        let html = render(&s, &Args::default());
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<meta name=\"viewport\""));
        assert!(html.contains("<style>"));
        assert!(html.contains("<title>Session 11111111"));
        assert!(html.contains("<h1>Session 11111111"));
        assert!(html.contains("<dt>Project</dt><dd>/tmp/proj_a</dd>"));
        assert!(html.contains("<dt>Git branch</dt><dd>main</dd>"));
        assert!(html.contains("<dt>Models</dt><dd>claude-sonnet-4-6</dd>"));
        assert!(html.contains("class=\"turn turn-user\""));
        assert!(html.contains("class=\"turn turn-assistant\""));
        assert!(html.ends_with("</html>\n") || html.ends_with("</html>"));
    }

    #[test]
    fn render_omits_git_branch_when_absent() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_thinking.jsonl")).unwrap();
        let html = render(&s, &Args::default());
        assert!(!html.contains("<dt>Git branch</dt>"));
    }

    #[test]
    fn render_skips_meta_entries() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_meta_only.jsonl")).unwrap();
        let html = render(&s, &Args::default());
        assert!(html.contains("<h1>Session 66666666"));
        assert!(!html.contains("class=\"turn turn-user\""));
        assert!(!html.contains("class=\"turn turn-assistant\""));
    }

    #[test]
    fn render_brackets_sidechain_spans() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_sidechain.jsonl")).unwrap();
        let html = render(&s, &Args::default());
        assert!(html.contains("class=\"subagent-banner\""));
        assert!(html.contains("🤖 Subagent task"));
        assert!(html.contains("← Resuming main thread"));
    }

    #[test]
    fn render_drops_sidechain_entries_when_disabled() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_sidechain.jsonl")).unwrap();
        let mut a = Args::default();
        a.sidechains = false;
        let html = render(&s, &a);
        assert!(!html.contains("class=\"subagent-banner\""));
        assert!(!html.contains("Subagent done"));
        assert!(!html.contains("subagent task"));
    }

    #[test]
    fn render_closes_trailing_sidechain_span() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_trailing_sidechain.jsonl")).unwrap();
        let html = render(&s, &Args::default());
        assert!(html.contains("🤖 Subagent task"));
        assert!(html.contains("← Resuming main thread"));
    }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test htmlexport::tests::render_emits_doctype_and_meta_header`
Expected: FAIL — `render` undefined.

- [ ] **Step 3: Implement `render`**

Add to `src/htmlexport.rs` above `#[cfg(test)]`. Add `use crate::mdexport::{Role, Session};` at the top of the file (or extend the existing `use crate::mdexport::...` import to include `Role` and `Session`):

```rust
use crate::mdexport::{Role, Session};
use chrono::Utc;

const EMBEDDED_CSS: &str = include_str!("htmlexport.css");

pub fn render(session: &Session, args: &Args) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!("<title>Session {}</title>\n", html_escape(&session.meta.id)));
    out.push_str("<style>\n");
    out.push_str(EMBEDDED_CSS);
    out.push_str("\n</style>\n</head>\n<body>\n");

    // Header card.
    out.push_str("<header class=\"meta\">\n");
    out.push_str(&format!("<h1>Session {}</h1>\n", html_escape(&session.meta.id)));
    out.push_str("<dl>\n");
    out.push_str(&format!("<dt>Project</dt><dd>{}</dd>\n", html_escape(&session.meta.project)));
    out.push_str(&format!("<dt>Started</dt><dd>{}</dd>\n", html_escape(&session.meta.started.to_rfc3339())));
    out.push_str(&format!("<dt>Ended</dt><dd>{}</dd>\n", html_escape(&session.meta.ended.to_rfc3339())));
    out.push_str(&format!("<dt>Messages</dt><dd>{}</dd>\n", session.meta.message_count));
    out.push_str(&format!("<dt>Models</dt><dd>{}</dd>\n", html_escape(&session.meta.models.join(", "))));
    if let Some(b) = &session.meta.git_branch {
        out.push_str(&format!("<dt>Git branch</dt><dd>{}</dd>\n", html_escape(b)));
    }
    out.push_str("</dl>\n</header>\n");

    // Conversation.
    out.push_str("<main class=\"conversation\">\n");

    let mut in_sidechain = false;
    for entry in &session.entries {
        if entry.is_meta { continue; }
        if entry.is_sidechain && !args.sidechains { continue; }

        if args.sidechains {
            if entry.is_sidechain && !in_sidechain {
                out.push_str("<div class=\"subagent-banner\">🤖 Subagent task</div>\n");
                in_sidechain = true;
            } else if !entry.is_sidechain && in_sidechain {
                out.push_str("<div class=\"subagent-banner end\">← Resuming main thread</div>\n");
                in_sidechain = false;
            }
        }

        let role_class = match entry.role { Role::User => "turn-user", Role::Assistant => "turn-assistant" };
        let role_name = match entry.role { Role::User => "User", Role::Assistant => "Assistant" };
        out.push_str(&format!("<section class=\"turn {}\">\n", role_class));
        out.push_str(&format!(
            "<header class=\"turn-head\"><span class=\"role\">{}</span><time>{}</time></header>\n",
            role_name,
            html_escape(&entry.timestamp.to_rfc3339())
        ));
        out.push_str("<div class=\"turn-body\">\n");
        for block in &entry.blocks {
            out.push_str(&render_block(block, args));
        }
        out.push_str("</div>\n</section>\n");
    }

    if in_sidechain {
        out.push_str("<div class=\"subagent-banner end\">← Resuming main thread</div>\n");
    }

    out.push_str("</main>\n");

    // Footer.
    out.push_str(&format!(
        "<footer class=\"footer\">Generated by oronzo {} · {}</footer>\n",
        env!("CARGO_PKG_VERSION"),
        html_escape(&Utc::now().to_rfc3339())
    ));

    out.push_str("</body>\n</html>\n");
    out
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test htmlexport::tests`
Expected: 20 tests pass (all earlier + 6 new).

- [ ] **Step 5: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): full document renderer with header, conversation, sidechain bracketing"
```

---

### Task 9: Wire `run()` end-to-end + `help()`

**Files:**
- Modify: `src/htmlexport.rs`

- [ ] **Step 1: Replace the `run` stub and add `help`**

In `src/htmlexport.rs`, find the existing `pub fn run(_args: &[String])` stub and replace it. Then add `pub fn help()` next to it.

The full replacement:

```rust
pub fn run(args: &[String]) {
    let parsed = match parse_args(args) {
        Ok(a) => a,
        Err(e) if e == "__help__" => {
            eprint!("{}", help());
            return;
        }
        Err(e) => {
            eprintln!("htmlexport: {e}\n\n{}", help());
            std::process::exit(2);
        }
    };

    // Translate to mdexport::Args for the selection helpers.
    let mdex_args = crate::mdexport::Args {
        query: parsed.query.clone(),
        tools: parsed.tools,
        thinking: parsed.thinking,
        sidechains: parsed.sidechains,
        images: parsed.images,
    };

    let selected = match crate::mdexport::select_session(&mdex_args) {
        Ok(Some(f)) => f,
        Ok(None) => return,
        Err(msg) => {
            eprintln!("htmlexport: {msg}");
            std::process::exit(1);
        }
    };

    let session = match crate::mdexport::parse_session(&selected.path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("htmlexport: cannot read {}: {e}", selected.path.display());
            std::process::exit(1);
        }
    };

    print!("{}", render(&session, &parsed));
}

pub fn help() -> String {
    "\
oronzo htmlexport: Export a Claude Code session as a self-contained HTML document.

Usage:
  oronzo htmlexport [query] [flags]

Selection:
  (no query)         Pick from the 30 most-recent sessions.
  <uuid-prefix>      ≥8 hex/dash chars → direct match (or picker if ambiguous).
  <words>            BM25 search; pick from up to top 30 results.

Flags:
  --no-tools         Drop tool_use and tool_result blocks.
  --no-thinking      Drop thinking blocks.
  --no-sidechains    Drop subagent entries.
  --no-images        Replace image blocks with a placeholder.
  -h, --help         Show this help.
".to_string()
}
```

- [ ] **Step 2: Verify**

Run: `cargo build`
Expected: clean build.

Run: `cargo test`
Expected: full suite green.

Run: `cargo run --quiet -- htmlexport --help 2>&1 | head -3`
Expected: starts with `oronzo htmlexport: Export a Claude Code session as a self-contained HTML document.`

Run: `cargo run --quiet -- htmlexport 99999999 2>&1 | head -1`
Expected: `htmlexport: no session matches prefix '99999999'`. Exit code 1.

- [ ] **Step 3: Commit**

```bash
git add src/htmlexport.rs
git commit -m "feat(htmlexport): wire end-to-end run() with selection and rendering"
```

---

### Task 10: Add `htmlexport` to top-level help

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update the help text**

In `src/main.rs::help_text()`, find the `Commands:` block. It currently has lines for `search`, `mdexport`, `usage`, ... Insert a new line right after the `mdexport` line:

The full updated block should read:

```
Commands:
  search <query>       Search and resume sessions
  mdexport [query]     Export a session as markdown
  htmlexport [query]   Export a session as HTML
  usage [report]       Token and cost reports (daily/weekly/monthly/session/blocks)
  account-switch       Interactive account switcher
  account-save         Save current account
  account-list         List saved accounts
  account-use <email>  Switch to a specific account
  mv <from> <to>       Move folder, keep sessions
  upgrade              Update to the latest version
```

- [ ] **Step 2: Smoke test**

Run: `cargo run --quiet -- --help 2>&1 | grep htmlexport`
Expected: `  htmlexport [query]   Export a session as HTML`

Run: `cargo test`
Expected: full suite still green.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "docs(htmlexport): surface new command in top-level help"
```

---

### Task 11: Integration test

**Files:**
- Create: `tests/htmlexport_e2e.rs`

- [ ] **Step 1: Write the test file**

Create `tests/htmlexport_e2e.rs`:

```rust
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mdexport/proj_a")
}

#[test]
fn export_sess_with_tools_to_html() {
    use oronzo::htmlexport::{render, Args};
    use oronzo::mdexport::parse_session;
    let session = parse_session(&fixtures().join("sess_with_tools.jsonl")).unwrap();
    let html = render(&session, &Args::default());

    // Document structure
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<title>Session 11111111-1111-1111-1111-111111111111</title>"));
    assert!(html.contains("<style>"));

    // Header card
    assert!(html.contains("<dt>Project</dt><dd>/tmp/proj_a</dd>"));
    assert!(html.contains("<dt>Git branch</dt><dd>main</dd>"));

    // Turns
    assert!(html.contains("class=\"turn turn-user\""));
    assert!(html.contains("class=\"turn turn-assistant\""));

    // Bash tool
    assert!(html.contains("class=\"tool tool-bash\""));
    assert!(html.contains("<em>List directory</em>"));
    assert!(html.contains("<pre class=\"code language-bash\">ls -la</pre>"));

    // Tool result text
    assert!(html.contains("class=\"result\""));
    assert!(html.contains("total 0"));

    // TodoWrite list
    assert!(html.contains("class=\"tool tool-todo\""));
    assert!(html.contains("<li class=\"todo-pending\">check tests</li>"));
    assert!(html.contains("<li class=\"todo-in-progress\">review code</li>"));
    assert!(html.contains("<li class=\"todo-done\">commit</li>"));
}

#[test]
fn export_meta_only_yields_empty_conversation() {
    use oronzo::htmlexport::{render, Args};
    use oronzo::mdexport::parse_session;
    let session = parse_session(&fixtures().join("sess_meta_only.jsonl")).unwrap();
    let html = render(&session, &Args::default());
    assert!(html.contains("<h1>Session 66666666"));
    assert!(!html.contains("class=\"turn turn-user\""));
    assert!(!html.contains("class=\"turn turn-assistant\""));
}

#[test]
fn flag_strips_tools_and_thinking() {
    use oronzo::htmlexport::{render, Args};
    use oronzo::mdexport::parse_session;
    let session = parse_session(&fixtures().join("sess_with_thinking.jsonl")).unwrap();
    let mut args = Args::default();
    args.thinking = false;
    let html = render(&session, &args);
    assert!(!html.contains("class=\"thinking\""));
    assert!(!html.contains("💭 Thinking"));
    // The text block ("Here is X.") survives.
    assert!(html.contains("Here is X."));
}

#[test]
fn output_is_self_contained() {
    use oronzo::htmlexport::{render, Args};
    use oronzo::mdexport::parse_session;
    let session = parse_session(&fixtures().join("sess_with_image.jsonl")).unwrap();
    let html = render(&session, &Args::default());
    // No external resources.
    assert!(!html.contains("<script"));
    assert!(!html.contains("<link rel=\"stylesheet\""));
    assert!(!html.contains("src=\"http"));
    assert!(!html.contains("src=\"https"));
    assert!(!html.contains("src=\"//"));
    // Image is inlined as data URL.
    assert!(html.contains("src=\"data:image/png;base64,"));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test htmlexport_e2e`
Expected: 4 tests pass.

- [ ] **Step 3: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add tests/htmlexport_e2e.rs
git commit -m "test(htmlexport): integration test against fixture tree"
```

---

### Task 12: README documentation

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Insert the new section**

In `README.md`, find the `### oronzo mdexport [query]` section. Right after its closing paragraph (before the `### oronzo usage [report]` section), insert this new section:

````markdown
### `oronzo htmlexport [query]`

Export a single Claude Code session to a self-contained HTML document on stdout. Same selection model as `mdexport`; same toggle flags.

```bash
oronzo htmlexport > out.html
oronzo htmlexport "fix auth bug" > bug.html
oronzo htmlexport 11111111 > session.html
oronzo htmlexport --no-tools --no-thinking
```

CSS is embedded inline; images go in as base64 `data:` URLs; no external resources, no JavaScript. Light + dark theme via `prefers-color-scheme`. Mobile-friendly down to 320px. Markdown in text blocks rendered via [pulldown-cmark](https://crates.io/crates/pulldown-cmark) (CommonMark + tables).

| Flag | Effect |
|---|---|
| `--no-tools` | drop `tool_use` and `tool_result` blocks |
| `--no-thinking` | drop `thinking` blocks |
| `--no-sidechains` | drop subagent (sidechain) entries |
| `--no-images` | replace image blocks with a placeholder |
````

- [ ] **Step 2: Verify**

Run: `cargo test`
Expected: still green (docs-only change).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(htmlexport): document the htmlexport command in README"
```

---

## Self-review notes

- **Spec coverage:**
  - Command surface + selection model (T2 args, T9 dispatch).
  - All 4 toggle flags (T2 parsing, T6/T7 effects, T8 sidechain handling).
  - Architecture and reuse from mdexport (T6 imports `Block`/`ToolResultContent`, T8 imports `Role`/`Session`/`parse_session`, T9 imports `Args`/`select_session`/`parse_session`).
  - Embedded CSS (T5 file + T8 `include_str!`).
  - `pulldown-cmark` dep (T1).
  - HTML escape helper (T3).
  - All block kinds (T6 + T7).
  - Sidechain bracketing including trailing-span flush (T8).
  - Header card + footer (T8, with `env!("CARGO_PKG_VERSION")` + `chrono::Utc::now`).
  - Error handling (T9 — missing dir, empty results, parse error, cancel).
  - Testing strategy (unit tests in T2-T8 + integration in T11).
  - README (T12).
  - Top-level help (T10).
- **Type consistency:** `Args`, `Block`, `ToolResultContent`, `Role`, `Session` all referenced with the same shapes across tasks. The `mdexport::Args` translation in T9 is the only place where the two `Args` types meet, and the field set is identical.
- **Placeholder scan:** every code step has concrete code; no "TBD" / "TODO" / "Similar to Task N" references.
- **Order independence:** T5 (CSS) can be done in parallel with T2-T4 since it's a separate file; for sequencing simplicity it's placed before T6 (where it's first needed).
- **Out of scope** (consistent with spec): no syntax highlighting, no dark-mode toggle button, no font loading, no `Args` refactor across mdexport/htmlexport.
