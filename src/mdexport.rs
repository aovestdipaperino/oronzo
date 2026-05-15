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

pub fn run(args: &[String]) {
    let parsed = match parse_args(args) {
        Ok(a) => a,
        Err(e) if e == "__help__" => {
            eprint!("{}", help());
            return;
        }
        Err(e) => {
            eprintln!("mdexport: {e}\n\n{}", help());
            std::process::exit(2);
        }
    };

    let selected = match select_session(&parsed) {
        Ok(Some(f)) => f,
        Ok(None) => return,
        Err(msg) => {
            eprintln!("mdexport: {msg}");
            std::process::exit(1);
        }
    };

    let session = match parse_session(&selected.path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mdexport: cannot read {}: {e}", selected.path.display());
            std::process::exit(1);
        }
    };

    print!("{}", render(&session, &parsed));
}

pub fn help() -> String {
    "\
oronzo mdexport: Export a Claude Code session as Markdown.

Usage:
  oronzo mdexport [query] [flags]

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
"
    .to_string()
}

use chrono::{DateTime, Utc};
use crate::sessions::{self, SessionFile};
use dirs;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, PartialEq, Clone)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Block {
    Text(String),
    Thinking(String),
    ToolUse {
        name: String,
        input: serde_json::Value,
        id: String,
    },
    ToolResult {
        content: ToolResultContent,
        is_error: bool,
    },
    Image {
        media_type: String,
        data: String,
    },
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub timestamp: DateTime<Utc>,
    pub role: Role,
    pub is_sidechain: bool,
    pub is_meta: bool,
    pub is_compact_summary: bool,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub path: std::path::PathBuf,
    pub id: String,
    pub project: String,
    pub first_msg: String,
    pub mtime: f64,
    pub score: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub project: String,
    pub git_branch: Option<String>,
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
    pub message_count: usize,
    pub models: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub meta: SessionMeta,
    pub entries: Vec<Entry>,
}

fn parse_blocks(v: &serde_json::Value) -> Vec<Block> {
    match v {
        serde_json::Value::String(s) => vec![Block::Text(s.clone())],
        serde_json::Value::Array(arr) => arr.iter().filter_map(parse_block).collect(),
        _ => Vec::new(),
    }
}

fn parse_block(v: &serde_json::Value) -> Option<Block> {
    let ty = v.get("type")?.as_str()?;
    match ty {
        "text" => v
            .get("text")
            .and_then(|t| t.as_str())
            .map(|s| Block::Text(s.to_string())),
        "thinking" => v
            .get("thinking")
            .and_then(|t| t.as_str())
            .map(|s| Block::Thinking(s.to_string())),
        "tool_use" => {
            let name = v.get("name")?.as_str()?.to_string();
            let input = v
                .get("input")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let id = v
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();
            Some(Block::ToolUse { name, input, id })
        }
        "tool_result" => {
            let is_error = v
                .get("is_error")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            let content = match v.get("content") {
                Some(serde_json::Value::String(s)) => ToolResultContent::Text(s.clone()),
                Some(serde_json::Value::Array(arr)) => {
                    let blocks: Vec<Block> = arr.iter().filter_map(parse_block).collect();
                    ToolResultContent::Blocks(blocks)
                }
                _ => ToolResultContent::Text(String::new()),
            };
            Some(Block::ToolResult { content, is_error })
        }
        "image" => {
            let source = v.get("source")?;
            let media_type = source
                .get("media_type")
                .and_then(|m| m.as_str())
                .unwrap_or("image/png")
                .to_string();
            let data = source
                .get("data")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            Some(Block::Image { media_type, data })
        }
        _ => None,
    }
}

pub fn parse_session(path: &Path) -> Result<Session, std::io::Error> {
    let file = fs::File::open(path)?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut id = String::new();
    let mut project = String::new();
    let mut git_branch: Option<String> = None;
    let mut started: Option<DateTime<Utc>> = None;
    let mut ended: Option<DateTime<Utc>> = None;
    let mut models: Vec<String> = Vec::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let ty = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty != "user" && ty != "assistant" {
            continue;
        }

        let Some(ts_str) = obj.get("timestamp").and_then(|t| t.as_str()) else {
            continue;
        };
        let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) else {
            continue;
        };
        let timestamp = ts.with_timezone(&Utc);

        if id.is_empty() {
            if let Some(s) = obj.get("sessionId").and_then(|s| s.as_str()) {
                id = s.to_string();
            }
        }
        if project.is_empty() {
            if let Some(c) = obj.get("cwd").and_then(|c| c.as_str()) {
                project = c.to_string();
            }
        }
        if git_branch.is_none() {
            if let Some(b) = obj.get("gitBranch").and_then(|b| b.as_str()) {
                git_branch = Some(b.to_string());
            }
        }
        started = Some(started.map_or(timestamp, |t| t.min(timestamp)));
        ended = Some(ended.map_or(timestamp, |t| t.max(timestamp)));

        if let Some(m) = obj
            .pointer("/message/model")
            .and_then(|m| m.as_str())
        {
            if !models.iter().any(|x| x == m) {
                models.push(m.to_string());
            }
        }

        let role = if ty == "user" {
            Role::User
        } else {
            Role::Assistant
        };
        let is_sidechain = obj
            .get("isSidechain")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let is_meta = obj
            .get("isMeta")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let is_compact_summary = obj
            .get("isCompactSummary")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);

        let content = obj
            .pointer("/message/content")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let blocks = parse_blocks(&content);

        entries.push(Entry {
            timestamp,
            role,
            is_sidechain,
            is_meta,
            is_compact_summary,
            blocks,
        });
    }

    let message_count = entries.iter().filter(|e| !e.is_meta).count();
    let started = started.unwrap_or_else(Utc::now);
    let ended = ended.unwrap_or_else(Utc::now);

    Ok(Session {
        meta: SessionMeta {
            id,
            project,
            git_branch,
            started,
            ended,
            message_count,
            models,
        },
        entries,
    })
}

pub fn render(session: &Session, args: &Args) -> String {
    let mut out = String::new();

    // Header table.
    out.push_str(&format!("# Session {}\n\n", session.meta.id));
    out.push_str(&format!("| Project | {} |\n", session.meta.project));
    out.push_str("|---|---|\n");
    out.push_str(&format!("| Started | {} |\n", session.meta.started.to_rfc3339()));
    out.push_str(&format!("| Ended | {} |\n", session.meta.ended.to_rfc3339()));
    out.push_str(&format!("| Messages | {} |\n", session.meta.message_count));
    out.push_str(&format!("| Models | {} |\n", session.meta.models.join(", ")));
    if let Some(b) = &session.meta.git_branch {
        out.push_str(&format!("| Git branch | {} |\n", b));
    }
    out.push_str("\n---\n\n");

    // Entries with sidechain bracketing.
    let mut in_sidechain = false;
    for entry in &session.entries {
        if entry.is_meta { continue; }
        if entry.is_sidechain && !args.sidechains { continue; }

        if args.sidechains {
            if entry.is_sidechain && !in_sidechain {
                out.push_str("### 🤖 Subagent task\n\n");
                in_sidechain = true;
            } else if !entry.is_sidechain && in_sidechain {
                out.push_str("### ← Resuming main thread\n\n");
                in_sidechain = false;
            }
        }

        let role = match entry.role { Role::User => "User", Role::Assistant => "Assistant" };
        out.push_str(&format!(
            "## {} · {}\n\n",
            role,
            entry.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        for block in &entry.blocks {
            out.push_str(&render_block(block, args));
        }
        out.push('\n');
    }

    if in_sidechain {
        out.push_str("### ← Resuming main thread\n\n");
    }

    out
}

pub fn lang_for_path(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext.to_lowercase().as_str() {
        "rs" => "rust",
        "py" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "go" => "go",
        "rb" => "ruby",
        "java" => "java",
        "kt" => "kotlin",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "hh" => "cpp",
        "sh" | "bash" => "bash",
        "zsh" => "zsh",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "html" => "html",
        "css" => "css",
        "sql" => "sql",
        "xml" => "xml",
        _ => "text",
    }
}

pub fn render_block(block: &Block, args: &Args) -> String {
    match block {
        Block::Text(s) => {
            let mut out = String::new();
            out.push_str(s);
            out.push('\n');
            out
        }
        Block::Thinking(s) => {
            if !args.thinking {
                return String::new();
            }
            format!("<details><summary>💭 Thinking</summary>\n\n{s}\n\n</details>\n")
        }
        Block::Image { media_type, data } => {
            if !args.images {
                return format!("_(image omitted: {}, {} bytes)_\n", media_type, data.len());
            }
            format!("![image](data:{};base64,{})\n", media_type, data)
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

fn render_tool_use(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let desc = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let mut out = String::new();
            if !desc.is_empty() {
                out.push_str(&format!("_{desc}_\n\n"));
            }
            out.push_str(&format!("```bash\n{cmd}\n```\n"));
            out
        }
        "Edit" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let old = input.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let new = input.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            let replace_all = input.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut out = format!("**Edit: `{path}`**");
            if replace_all { out.push_str(" _(replace_all)_"); }
            out.push_str("\n\n```diff\n");
            for line in old.lines() { out.push_str(&format!("- {line}\n")); }
            for line in new.lines() { out.push_str(&format!("+ {line}\n")); }
            out.push_str("```\n");
            out
        }
        "Write" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let lang = lang_for_path(path);
            format!("**Write: `{path}`**\n\n```{lang}\n{content}\n```\n")
        }
        "Read" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let offset = input.get("offset").and_then(|v| v.as_u64());
            let limit = input.get("limit").and_then(|v| v.as_u64());
            let mut out = format!("**Read: `{path}`**");
            if offset.is_some() || limit.is_some() {
                let o = offset.unwrap_or(0);
                let l = limit.unwrap_or(0);
                out.push_str(&format!(" _(offset {o}, limit {l})_"));
            }
            out.push('\n');
            out
        }
        "TodoWrite" => {
            let mut out = String::new();
            if let Some(todos) = input.get("todos").and_then(|v| v.as_array()) {
                for todo in todos {
                    let content = todo.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let status = todo.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let line = match status {
                        "completed" => format!("- [x] {content}\n"),
                        "in_progress" => format!("- [ ] 🚧 {content}\n"),
                        _ => format!("- [ ] {content}\n"),
                    };
                    out.push_str(&line);
                }
            }
            out
        }
        other => {
            let json = serde_json::to_string_pretty(input).unwrap_or_else(|_| "{}".into());
            format!("**Tool: {other}**\n\n```json\n{json}\n```\n")
        }
    }
}

fn render_tool_result(content: &ToolResultContent, is_error: bool, args: &Args) -> String {
    let mut out = String::new();
    if is_error { out.push_str("**❌ Tool error:**\n\n"); }
    match content {
        ToolResultContent::Text(s) => {
            out.push_str(&format!("```text\n{s}\n```\n"));
        }
        ToolResultContent::Blocks(blocks) => {
            for b in blocks {
                out.push_str(&render_block(b, args));
            }
        }
    }
    out
}

pub fn looks_like_uuid_prefix(s: &str) -> bool {
    if s.len() < 8 { return false; }
    s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

pub fn resolve_uuid_prefix(claude_dir: &Path, prefix: &str) -> Vec<SessionFile> {
    sessions::discover(claude_dir)
        .into_iter()
        .filter(|s| s.id.starts_with(prefix))
        .collect()
}

pub fn list_recent_sessions(claude_dir: &Path, limit: usize) -> Vec<SessionInfo> {
    let files = sessions::discover(claude_dir);
    let mut infos: Vec<SessionInfo> = files
        .into_iter()
        .filter_map(|sf| {
            let mtime = fs::metadata(&sf.path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let session = parse_session(&sf.path).ok()?;
            let first_msg = first_user_text(&session).unwrap_or_else(|| "(no summary)".into());
            Some(SessionInfo {
                path: sf.path,
                id: sf.id,
                project: session.meta.project,
                first_msg,
                mtime,
                score: None,
            })
        })
        .collect();
    infos.sort_by(|a, b| b.mtime.partial_cmp(&a.mtime).unwrap_or(std::cmp::Ordering::Equal));
    infos.truncate(limit);
    infos
}

fn first_user_text(s: &Session) -> Option<String> {
    for e in &s.entries {
        if e.is_meta { continue; }
        if !matches!(e.role, Role::User) { continue; }
        for b in &e.blocks {
            if let Block::Text(t) = b {
                let cleaned: String = t.replace('\n', " ");
                let truncated: String = cleaned.chars().take(90).collect();
                if truncated.chars().count() < cleaned.chars().count() {
                    return Some(format!("{truncated}…"));
                }
                return Some(truncated);
            }
        }
    }
    None
}

pub fn format_picker_line(idx: usize, info: &SessionInfo, home: &str) -> String {
    let project = if !home.is_empty() && info.project.starts_with(home) {
        info.project.replacen(home, "~", 1)
    } else {
        info.project.clone()
    };
    let when = chrono::DateTime::<chrono::Local>::from(
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs_f64(info.mtime),
    )
    .format("%Y-%m-%d %H:%M");
    if let Some(score) = info.score {
        format!("  {idx:>2}. [{score:.4}] {}  ({}, {when})\n", info.first_msg, project)
    } else {
        format!("  {idx:>2}. {}  ({}, {when})\n", info.first_msg, project)
    }
}

const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn score_bm25(query: &str, corpus: &[String]) -> Vec<f64> {
    let n = corpus.len() as f64;
    let tokenized: Vec<Vec<String>> = corpus.iter().map(|d| tokenize(d)).collect();
    let doc_lens: Vec<f64> = tokenized.iter().map(|t| t.len() as f64).collect();
    let avgdl = if tokenized.is_empty() { 1.0 } else { doc_lens.iter().sum::<f64>() / n };
    let mut df: HashMap<String, f64> = HashMap::new();
    for doc in &tokenized {
        let unique: std::collections::HashSet<&String> = doc.iter().collect();
        for t in unique { *df.entry(t.clone()).or_default() += 1.0; }
    }
    let qtoks = tokenize(query);
    tokenized
        .iter()
        .enumerate()
        .map(|(i, doc)| {
            let dl = doc_lens[i];
            let mut tf: HashMap<&str, f64> = HashMap::new();
            for t in doc { *tf.entry(t.as_str()).or_default() += 1.0; }
            qtoks.iter().fold(0.0, |s, q| {
                let dfq = df.get(q.as_str()).copied().unwrap_or(0.0);
                let tfq = tf.get(q.as_str()).copied().unwrap_or(0.0);
                let idf = ((n - dfq + 0.5) / (dfq + 0.5) + 1.0).ln();
                let comp = (tfq * (BM25_K1 + 1.0)) / (tfq + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl));
                s + idf * comp
            })
        })
        .collect()
}

fn collect_text(s: &Session) -> String {
    let mut buf = String::new();
    for e in &s.entries {
        if e.is_meta { continue; }
        for b in &e.blocks {
            if let Block::Text(t) = b {
                buf.push_str(t);
                buf.push(' ');
            }
        }
    }
    buf
}

pub fn rank_with_query(claude_dir: &Path, query: &str) -> Vec<SessionInfo> {
    let infos = list_recent_sessions(claude_dir, usize::MAX);
    let corpus: Vec<String> = infos
        .iter()
        .map(|i| parse_session(&i.path).map(|s| collect_text(&s)).unwrap_or_default())
        .collect();
    let scores = score_bm25(query, &corpus);
    let mut ranked: Vec<SessionInfo> = infos
        .into_iter()
        .zip(scores)
        .filter(|(_, sc)| *sc > 0.0)
        .map(|(mut info, sc)| { info.score = Some(sc); info })
        .collect();
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(30);
    ranked
}

pub fn parse_selection(input: &str, count: usize) -> Option<usize> {
    let trimmed = input.trim();
    if trimmed.is_empty() { return None; }
    match trimmed.parse::<usize>() {
        Ok(n) if n >= 1 && n <= count => Some(n - 1),
        _ => None,
    }
}

pub fn prompt_selection(count: usize) -> Option<usize> {
    eprint!("Select number (Enter to cancel): ");
    let _ = std::io::stderr().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return None;
    }
    parse_selection(&input, count)
}

pub fn select_session(args: &Args) -> Result<Option<SessionFile>, String> {
    let claude_dir = sessions::claude_dir();
    if !claude_dir.exists() {
        return Err(format!("Claude sessions directory not found at {}", claude_dir.display()));
    }

    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    match &args.query {
        None => {
            let infos = list_recent_sessions(&claude_dir, 30);
            if infos.is_empty() {
                return Err("no sessions found".into());
            }
            Ok(pick_from(&infos, &home))
        }
        Some(q) if looks_like_uuid_prefix(q) => {
            let matches = resolve_uuid_prefix(&claude_dir, q);
            match matches.len() {
                0 => Err(format!("no session matches prefix '{q}'")),
                1 => Ok(Some(matches.into_iter().next().unwrap())),
                _ => {
                    let infos: Vec<SessionInfo> = matches
                        .into_iter()
                        .filter_map(|sf| {
                            let mtime = fs::metadata(&sf.path)
                                .and_then(|m| m.modified())
                                .ok()
                                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs_f64())
                                .unwrap_or(0.0);
                            let session = parse_session(&sf.path).ok()?;
                            let first_msg = first_user_text(&session).unwrap_or_else(|| "(no summary)".into());
                            Some(SessionInfo {
                                path: sf.path,
                                id: sf.id,
                                project: session.meta.project,
                                first_msg,
                                mtime,
                                score: None,
                            })
                        })
                        .collect();
                    Ok(pick_from(&infos, &home))
                }
            }
        }
        Some(q) => {
            let infos = rank_with_query(&claude_dir, q);
            if infos.is_empty() {
                return Err(format!("no results for '{q}'"));
            }
            Ok(pick_from(&infos, &home))
        }
    }
}

fn pick_from(infos: &[SessionInfo], home: &str) -> Option<SessionFile> {
    eprintln!();
    for (i, info) in infos.iter().enumerate() {
        eprint!("{}", format_picker_line(i + 1, info, home));
    }
    let idx = prompt_selection(infos.len())?;
    let info = &infos[idx];
    Some(SessionFile { id: info.id.clone(), path: info.path.clone() })
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
        assert!(a.query.is_none());
        assert!(a.tools && a.thinking && a.sidechains && a.images);
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
    fn flags_and_query_mix() {
        let a = parse_args(&argv(&["--no-tools", "fix", "bug"])).unwrap();
        assert!(!a.tools);
        assert_eq!(a.query.as_deref(), Some("fix bug"));
    }

    #[test]
    fn help_returns_sentinel_error() {
        let e = parse_args(&argv(&["--help"])).unwrap_err();
        assert_eq!(e, "__help__");
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(parse_args(&argv(&["--bogus"])).is_err());
    }

    fn fixture(rel: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn parse_session_with_tools_extracts_blocks() {
        let s = parse_session(&fixture(
            "tests/fixtures/mdexport/proj_a/sess_with_tools.jsonl",
        ))
        .unwrap();
        assert_eq!(s.meta.id, "11111111-1111-1111-1111-111111111111");
        assert_eq!(s.meta.project, "/tmp/proj_a");
        assert_eq!(s.meta.git_branch.as_deref(), Some("main"));
        assert_eq!(s.meta.models, vec!["claude-sonnet-4-6"]);
        assert_eq!(s.entries.len(), 4);
        assert!(matches!(s.entries[0].role, Role::User));
        assert_eq!(s.entries[1].blocks.len(), 2);
        assert!(matches!(s.entries[1].blocks[0], Block::Text(_)));
        assert!(matches!(s.entries[1].blocks[1], Block::ToolUse { .. }));
        assert_eq!(s.entries[2].blocks.len(), 1);
        assert!(matches!(s.entries[2].blocks[0], Block::ToolResult { .. }));
    }

    #[test]
    fn parse_session_with_thinking() {
        let s = parse_session(&fixture(
            "tests/fixtures/mdexport/proj_a/sess_with_thinking.jsonl",
        ))
        .unwrap();
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[1].blocks.len(), 2);
        assert!(matches!(s.entries[1].blocks[0], Block::Thinking(_)));
        assert!(matches!(s.entries[1].blocks[1], Block::Text(_)));
    }

    #[test]
    fn parse_session_with_sidechain_flags() {
        let s = parse_session(&fixture(
            "tests/fixtures/mdexport/proj_a/sess_with_sidechain.jsonl",
        ))
        .unwrap();
        assert_eq!(s.entries.len(), 5);
        assert!(!s.entries[0].is_sidechain);
        assert!(!s.entries[1].is_sidechain);
        assert!(s.entries[2].is_sidechain);
        assert!(s.entries[3].is_sidechain);
        assert!(!s.entries[4].is_sidechain);
    }

    #[test]
    fn parse_session_with_image_extracts_data() {
        let s = parse_session(&fixture(
            "tests/fixtures/mdexport/proj_a/sess_with_image.jsonl",
        ))
        .unwrap();
        assert_eq!(s.entries[0].blocks.len(), 1);
        if let Block::Image { media_type, data } = &s.entries[0].blocks[0] {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBORw0KGgo=");
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn parse_session_meta_only_counts_zero() {
        let s = parse_session(&fixture(
            "tests/fixtures/mdexport/proj_a/sess_meta_only.jsonl",
        ))
        .unwrap();
        assert!(s.entries.iter().all(|e| e.is_meta));
        assert_eq!(s.meta.message_count, 0);
    }

    #[test]
    fn lang_for_extension_maps_known_languages() {
        assert_eq!(lang_for_path("foo.rs"), "rust");
        assert_eq!(lang_for_path("/a/b/foo.py"), "python");
        assert_eq!(lang_for_path("script.sh"), "bash");
        assert_eq!(lang_for_path("data.json"), "json");
        assert_eq!(lang_for_path("config.yaml"), "yaml");
        assert_eq!(lang_for_path("page.tsx"), "typescript");
        assert_eq!(lang_for_path("page.jsx"), "javascript");
    }

    #[test]
    fn lang_for_unknown_extension_is_text() {
        assert_eq!(lang_for_path("foo.unknownext"), "text");
        assert_eq!(lang_for_path("noext"), "text");
        assert_eq!(lang_for_path("/some/dir/"), "text");
    }

    #[test]
    fn render_text_block_emits_text_as_is() {
        let out = render_block(&Block::Text("hello **world**".into()), &Args::default());
        assert_eq!(out, "hello **world**\n");
    }

    #[test]
    fn render_thinking_uses_details() {
        let out = render_block(&Block::Thinking("step 1\nstep 2".into()), &Args::default());
        assert!(out.starts_with("<details><summary>💭 Thinking</summary>"));
        assert!(out.contains("step 1\nstep 2"));
        assert!(out.trim_end().ends_with("</details>"));
    }

    #[test]
    fn render_thinking_skipped_when_disabled() {
        let mut a = Args::default();
        a.thinking = false;
        let out = render_block(&Block::Thinking("anything".into()), &a);
        assert!(out.is_empty());
    }

    #[test]
    fn render_image_emits_data_url() {
        let block = Block::Image { media_type: "image/png".into(), data: "iVBORw0KGgo=".into() };
        let out = render_block(&block, &Args::default());
        assert_eq!(out.trim(), "![image](data:image/png;base64,iVBORw0KGgo=)");
    }

    #[test]
    fn render_image_omitted_when_disabled() {
        let mut a = Args::default();
        a.images = false;
        let block = Block::Image { media_type: "image/png".into(), data: "iVBORw0KGgo=".into() };
        let out = render_block(&block, &a);
        assert!(out.contains("(image omitted: image/png"));
    }

    fn tool_use(name: &str, input: serde_json::Value) -> Block {
        Block::ToolUse { name: name.into(), input, id: "t-1".into() }
    }

    #[test]
    fn render_bash_emits_bash_code_block() {
        let b = tool_use("Bash", serde_json::json!({"command":"ls -la","description":"list"}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("_list_"));
        assert!(out.contains("```bash\nls -la\n```"));
    }

    #[test]
    fn render_edit_emits_diff_block() {
        let b = tool_use("Edit", serde_json::json!({
            "file_path":"/tmp/x.rs", "old_string":"foo", "new_string":"bar", "replace_all": true,
        }));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("**Edit: `/tmp/x.rs`**"));
        assert!(out.contains("```diff"));
        assert!(out.contains("- foo"));
        assert!(out.contains("+ bar"));
        assert!(out.contains("_(replace_all)_"));
    }

    #[test]
    fn render_write_picks_language() {
        let b = tool_use("Write", serde_json::json!({"file_path":"/tmp/a.rs","content":"fn main(){}"}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("**Write: `/tmp/a.rs`**"));
        assert!(out.contains("```rust\nfn main(){}\n```"));
    }

    #[test]
    fn render_read_with_offset_and_limit() {
        let b = tool_use("Read", serde_json::json!({"file_path":"/tmp/a.rs","offset":10,"limit":50}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("**Read: `/tmp/a.rs`**"));
        assert!(out.contains("_(offset 10, limit 50)_"));
    }

    #[test]
    fn render_todowrite_renders_checklist() {
        let b = tool_use("TodoWrite", serde_json::json!({
            "todos":[
                {"content":"alpha","status":"pending"},
                {"content":"beta","status":"in_progress"},
                {"content":"gamma","status":"completed"}
            ]
        }));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("- [ ] alpha"));
        assert!(out.contains("- [ ] 🚧 beta"));
        assert!(out.contains("- [x] gamma"));
    }

    #[test]
    fn render_other_tool_falls_back_to_json() {
        let b = tool_use("Glob", serde_json::json!({"pattern":"*.rs"}));
        let out = render_block(&b, &Args::default());
        assert!(out.contains("**Tool: Glob**"));
        assert!(out.contains("```json"));
        assert!(out.contains("\"pattern\": \"*.rs\""));
    }

    #[test]
    fn render_tool_result_text() {
        let b = Block::ToolResult { content: ToolResultContent::Text("hello".into()), is_error: false };
        let out = render_block(&b, &Args::default());
        assert!(out.contains("```text\nhello\n```"));
    }

    #[test]
    fn render_tool_result_error_prepends_marker() {
        let b = Block::ToolResult { content: ToolResultContent::Text("boom".into()), is_error: true };
        let out = render_block(&b, &Args::default());
        assert!(out.contains("**❌ Tool error:**"));
        assert!(out.contains("```text\nboom\n```"));
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

    #[test]
    fn render_emits_header_with_id_and_project() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_tools.jsonl")).unwrap();
        let md = render(&s, &Args::default());
        assert!(md.starts_with("# Session 11111111-1111-1111-1111-111111111111"));
        assert!(md.contains("| Project | /tmp/proj_a |"));
        assert!(md.contains("| Messages | 4 |"));
        assert!(md.contains("| Git branch | main |"));
        assert!(md.contains("| Models | claude-sonnet-4-6 |"));
    }

    #[test]
    fn render_omits_git_branch_when_absent() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_thinking.jsonl")).unwrap();
        let md = render(&s, &Args::default());
        assert!(!md.contains("| Git branch |"));
    }

    #[test]
    fn render_skips_meta_entries() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_meta_only.jsonl")).unwrap();
        let md = render(&s, &Args::default());
        assert!(md.contains("# Session 66666666"));
        assert!(!md.contains("## User"));
        assert!(!md.contains("## Assistant"));
    }

    #[test]
    fn render_brackets_sidechain_spans() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_sidechain.jsonl")).unwrap();
        let md = render(&s, &Args::default());
        assert!(md.contains("### 🤖 Subagent task"));
        assert!(md.contains("### ← Resuming main thread"));
    }

    #[test]
    fn render_drops_sidechain_entries_when_disabled() {
        let s = parse_session(&fixture("tests/fixtures/mdexport/proj_a/sess_with_sidechain.jsonl")).unwrap();
        let mut a = Args::default();
        a.sidechains = false;
        let md = render(&s, &a);
        assert!(!md.contains("subagent task"));
        assert!(!md.contains("Subagent done"));
        assert!(!md.contains("### 🤖 Subagent task"));
        assert!(!md.contains("### ← Resuming main thread"));
    }

    #[test]
    fn looks_like_uuid_prefix_recognizes_hex_and_dashes() {
        assert!(looks_like_uuid_prefix("11111111"));
        assert!(looks_like_uuid_prefix("11111111-1111"));
        assert!(looks_like_uuid_prefix("aabbccdd-eeff-1234-5678-90abcdef1234"));
        assert!(!looks_like_uuid_prefix("1234567"));            // too short
        assert!(!looks_like_uuid_prefix("not a uuid"));         // has space
        assert!(!looks_like_uuid_prefix("11111111-zzzz"));      // non-hex
    }

    #[test]
    fn resolve_uuid_prefix_finds_exact_match() {
        let fixtures_root = fixture("tests/fixtures/mdexport");
        let matches = resolve_uuid_prefix(&fixtures_root, "11111111");
        assert_eq!(matches.len(), 1);
        assert!(matches[0].path.to_string_lossy().ends_with("sess_with_tools.jsonl"));
    }

    #[test]
    fn resolve_uuid_prefix_no_match_returns_empty() {
        let fixtures_root = fixture("tests/fixtures/mdexport");
        let matches = resolve_uuid_prefix(&fixtures_root, "99999999");
        assert!(matches.is_empty());
    }

    #[test]
    fn list_recent_sessions_returns_session_info_with_first_msg() {
        let fixtures_root = fixture("tests/fixtures/mdexport");
        let infos = list_recent_sessions(&fixtures_root, 100);
        assert!(infos.len() >= 6);
        let tools = infos.iter().find(|i| i.id == "11111111-1111-1111-1111-111111111111").unwrap();
        assert_eq!(tools.project, "/tmp/proj_a");
        assert_eq!(tools.first_msg, "list the files");
    }

    #[test]
    fn format_picker_line_includes_score_when_present() {
        let info = SessionInfo {
            path: "/x".into(),
            id: "abcd".into(),
            project: "/Users/me/Code/foo".into(),
            first_msg: "do thing".into(),
            mtime: 1700000000.0,
            score: Some(0.5432),
        };
        let line = format_picker_line(1, &info, "/Users/me");
        assert!(line.contains("[0.5432]"));
        assert!(line.contains("~/Code/foo"));
        assert!(line.contains("do thing"));
    }

    #[test]
    fn format_picker_line_no_score_omits_brackets() {
        let info = SessionInfo {
            path: "/x".into(),
            id: "abcd".into(),
            project: "/tmp/p".into(),
            first_msg: "do thing".into(),
            mtime: 1700000000.0,
            score: None,
        };
        let line = format_picker_line(2, &info, "");
        assert!(!line.contains("["));
        assert!(line.contains("do thing"));
    }

    #[test]
    fn rank_with_query_orders_by_relevance() {
        let fixtures_root = fixture("tests/fixtures/mdexport");
        let infos = rank_with_query(&fixtures_root, "list the files");
        assert!(!infos.is_empty());
        assert_eq!(infos[0].id, "11111111-1111-1111-1111-111111111111");
        assert!(infos[0].score.unwrap() > 0.0);
    }

    #[test]
    fn parse_selection_valid_returns_index() {
        assert_eq!(parse_selection("1", 5), Some(0));
        assert_eq!(parse_selection("5", 5), Some(4));
    }

    #[test]
    fn parse_selection_invalid_returns_none() {
        assert_eq!(parse_selection("", 5), None);
        assert_eq!(parse_selection("0", 5), None);
        assert_eq!(parse_selection("6", 5), None);
        assert_eq!(parse_selection("abc", 5), None);
    }
}
