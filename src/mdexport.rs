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
    eprintln!("mdexport: not yet implemented");
    std::process::exit(2);
}

use chrono::{DateTime, Utc};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

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
}
