use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::process;
use std::time::SystemTime;

mod mv;
mod sessions;
mod switch;
mod upgrade;
mod usage;

const LOGO: &str = include_str!(concat!(env!("OUT_DIR"), "/logo.ansi"));

fn help_text() -> String {
    format!(
        "\
oronzo {}: A toolkit for Claude Code sessions.

Usage:
  oronzo <command> [args...]

Commands:
  search <query>       Search and resume sessions
  account-switch       Interactive account switcher
  account-save         Save current account
  account-list         List saved accounts
  account-use <email>  Switch to a specific account
  mv <from> <to>       Move folder, keep sessions
  upgrade              Update to the latest version

Options:
  -h, --help       Show this help
  -V, --version    Show version",
        env!("CARGO_PKG_VERSION")
    )
}

fn help() -> String {
    let text = help_text();
    let text_lines: Vec<&str> = text.lines().collect();
    let logo_lines: Vec<&str> = LOGO.lines().collect();
    let max_rows = text_lines.len().max(logo_lines.len());
    let col: usize = text_lines.iter().map(|l: &&str| l.len()).max().unwrap_or(0) + 2;
    let mut out = String::new();
    for i in 0..max_rows {
        let text = *text_lines.get(i).unwrap_or(&"");
        let pad = col.saturating_sub(text.len()).max(2);
        out.push_str(text);
        (0..pad).for_each(|_| out.push(' '));
        if let Some(logo_line) = logo_lines.get(i) {
            out.push_str(logo_line);
        }
        out.push('\n');
    }
    out
}

const SEARCH_USAGE: &str = "\
oronzo search: Search across Claude Code sessions and resume them.

Usage:
  oronzo search <query>
  oronzo search \"location history cluster\"
";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        eprint!("{}", help());
        process::exit(0);
    }

    if args[1] == "-V" || args[1] == "--version" {
        eprintln!("oronzo {}", env!("CARGO_PKG_VERSION"));
        process::exit(0);
    }

    // Non-blocking version check (500ms timeout, silent on failure)
    if args[1] != "upgrade" {
        upgrade::check_for_update();
    }

    match args[1].as_str() {
        "search" => cmd_search(&args[2..]),
        "mv" => mv::run(&args[2..]),
        "upgrade" => upgrade::run(),
        "account-switch" | "account-save" | "account-list" | "account-use" => {
            switch::run(&args[1..]);
        }
        other => {
            eprintln!("Unknown command: {other}\n");
            eprint!("{}", help_text());
            process::exit(1);
        }
    }
}

fn cmd_search(args: &[String]) {
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        eprint!("{SEARCH_USAGE}");
        process::exit(0);
    }

    let query = args.join(" ");

    let claude_dir = sessions::claude_dir();
    if !claude_dir.exists() {
        eprintln!(
            "Claude sessions directory not found: {}",
            claude_dir.display()
        );
        process::exit(1);
    }

    let session_files = sessions::discover(&claude_dir);
    if session_files.is_empty() {
        eprintln!("No sessions found.");
        process::exit(1);
    }

    let mut cache = Cache::new(Cache::default_path());
    let mut updated = false;

    struct Session {
        id: String,
        text: String,
        cwd: String,
        first_msg: String,
    }

    let mut sessions: Vec<Session> = Vec::new();

    for sf in &session_files {
        let key = sf.path.to_string_lossy().to_string();
        let mtime = file_mtime(&sf.path);

        if let Some(entry) = cache.get(&key, mtime) {
            if !entry.text.trim().is_empty() {
                sessions.push(Session {
                    id: sf.id.clone(),
                    text: entry.text.clone(),
                    cwd: entry.cwd.clone(),
                    first_msg: entry.first_msg.clone(),
                });
            }
        } else {
            let extracted = extract_session(&sf.path);
            let cwd = if extracted.cwd.is_empty() {
                sf.path
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                extracted.cwd.clone()
            };
            cache.set(
                key,
                CacheEntry {
                    mtime,
                    text: extracted.text.clone(),
                    cwd: cwd.clone(),
                    first_msg: extracted.first_msg.clone(),
                },
            );
            updated = true;
            if !extracted.text.trim().is_empty() {
                sessions.push(Session {
                    id: sf.id.clone(),
                    text: extracted.text,
                    cwd,
                    first_msg: extracted.first_msg,
                });
            }
        }
    }

    if updated {
        cache.save();
    }

    if sessions.is_empty() {
        eprintln!("No sessions found.");
        process::exit(1);
    }

    eprintln!("Indexing {} sessions ...", sessions.len());

    let corpus: Vec<String> = sessions.iter().map(|s| s.text.clone()).collect();
    let scores = score_bm25(&query, &corpus);

    let mut ranked: Vec<RankedSession> = scores
        .into_iter()
        .zip(sessions.into_iter())
        .filter(|(score, _)| *score > 0.0)
        .map(|(score, session)| RankedSession {
            score,
            id: session.id,
            cwd: session.cwd,
            first_msg: session.first_msg,
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.truncate(MAX_RESULTS);

    if ranked.is_empty() {
        eprintln!("No results found.");
        process::exit(1);
    }

    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    display_results(&ranked, &query, &home);

    if let Some(idx) = read_selection(ranked.len()) {
        let session = &ranked[idx];
        resume(&session.id, &session.cwd);
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use std::io::{BufRead, BufReader};

struct ExtractedSession {
    text: String,
    cwd: String,
    first_msg: String,
}

fn content_to_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|block| {
                if block.get("type")?.as_str()? == "text" {
                    block.get("text")?.as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

fn extract_session(filepath: &Path) -> ExtractedSession {
    let mut texts = Vec::new();
    let mut cwd = String::new();
    let mut first_msg = String::new();

    let Ok(file) = fs::File::open(filepath) else {
        return ExtractedSession {
            text: String::new(),
            cwd,
            first_msg,
        };
    };
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if obj.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        let text = content_to_text(
            obj.pointer("/message/content")
                .unwrap_or(&serde_json::Value::Null),
        );
        if !text.is_empty() {
            if first_msg.is_empty() {
                first_msg = text.clone();
            }
            texts.push(text);
        }
        if cwd.is_empty() {
            if let Some(c) = obj.get("cwd").and_then(|v| v.as_str()) {
                cwd = c.to_string();
            }
        }
    }

    ExtractedSession {
        text: texts.join(" "),
        cwd,
        first_msg,
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct CacheEntry {
    mtime: f64,
    text: String,
    cwd: String,
    first_msg: String,
}

struct Cache {
    path: PathBuf,
    entries: HashMap<String, CacheEntry>,
}

impl Cache {
    fn new(path: PathBuf) -> Self {
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Cache { path, entries }
    }

    fn default_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-search")
            .join("index.json")
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(
            &self.path,
            serde_json::to_string(&self.entries).unwrap_or_default(),
        );
    }

    fn get(&self, key: &str, mtime: f64) -> Option<&CacheEntry> {
        self.entries.get(key).filter(|e| e.mtime == mtime)
    }

    fn set(&mut self, key: String, entry: CacheEntry) {
        self.entries.insert(key, entry);
    }
}

fn file_mtime(path: &Path) -> f64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;

fn score_bm25(query: &str, corpus: &[String]) -> Vec<f64> {
    let n = corpus.len() as f64;
    let tokenized: Vec<Vec<String>> = corpus.iter().map(|doc| tokenize(doc)).collect();
    let doc_lens: Vec<f64> = tokenized.iter().map(|t| t.len() as f64).collect();
    let avgdl = if tokenized.is_empty() {
        1.0
    } else {
        doc_lens.iter().sum::<f64>() / n
    };

    // Document frequency for each term
    let mut df: HashMap<String, f64> = HashMap::new();
    for doc_tokens in &tokenized {
        let unique: std::collections::HashSet<&String> = doc_tokens.iter().collect();
        for term in unique {
            *df.entry(term.clone()).or_default() += 1.0;
        }
    }

    let query_tokens = tokenize(query);

    tokenized
        .iter()
        .enumerate()
        .map(|(i, doc_tokens)| {
            let dl = doc_lens[i];
            let mut tf: HashMap<&str, f64> = HashMap::new();
            for t in doc_tokens {
                *tf.entry(t.as_str()).or_default() += 1.0;
            }

            query_tokens.iter().fold(0.0, |score, qt| {
                let term_df = df.get(qt.as_str()).copied().unwrap_or(0.0);
                let term_tf = tf.get(qt.as_str()).copied().unwrap_or(0.0);
                let idf = ((n - term_df + 0.5) / (term_df + 0.5) + 1.0).ln();
                let tf_component = (term_tf * (BM25_K1 + 1.0))
                    / (term_tf + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl));
                score + idf * tf_component
            })
        })
        .collect()
}

const MAX_RESULTS: usize = 30;

struct RankedSession {
    score: f64,
    id: String,
    cwd: String,
    first_msg: String,
}

fn format_result_line(
    index: usize,
    score: f64,
    first_msg: &str,
    cwd: &str,
    session_id: &str,
) -> String {
    let label: String = first_msg.chars().take(90).collect();
    let label = label.replace('\n', " ");
    let short_id = if session_id.len() > 8 {
        format!("{}...", &session_id[..8])
    } else {
        session_id.to_string()
    };
    format!("  {index:2}. [{score:.3}] {label}\n       {cwd}  ({short_id})\n")
}

fn display_results(ranked: &[RankedSession], query: &str, home: &str) {
    eprintln!("\nFound {} results [BM25] for: '{}'\n", ranked.len(), query);
    for (i, session) in ranked.iter().enumerate() {
        let cwd = session.cwd.replace(home, "~");
        eprint!(
            "{}",
            format_result_line(i + 1, session.score, &session.first_msg, &cwd, &session.id)
        );
    }
}

fn read_selection(count: usize) -> Option<usize> {
    eprint!("Select number (Enter to cancel): ");
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return None;
    }
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= count => Some(n - 1),
        _ => {
            eprintln!("Invalid selection.");
            None
        }
    }
}

fn resume(session_id: &str, cwd: &str) {
    eprintln!("\nResuming {session_id}");
    eprintln!("Directory: {cwd}\n");

    if std::env::set_current_dir(cwd).is_err() {
        eprintln!("Warning: could not chdir to {cwd}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new("claude")
            .args(["--resume", session_id])
            .exec();
        eprintln!("Failed to exec claude: {err}");
        process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let status = std::process::Command::new("claude")
            .args(["--resume", session_id])
            .status();
        match status {
            Ok(s) => process::exit(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("Failed to run claude: {e}");
                process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_session_string_content() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/proj_a/session1.jsonl");
        let result = extract_session(&fixture);
        assert_eq!(result.cwd, "/tmp/proj_a");
        assert_eq!(result.first_msg, "hello world");
        assert!(result.text.contains("hello world"));
        assert!(result.text.contains("search for files"));
        assert!(!result.text.contains("hi there"));
    }

    #[test]
    fn test_extract_session_array_content() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/proj_b/session2.jsonl");
        let result = extract_session(&fixture);
        assert_eq!(result.cwd, "/tmp/proj_b");
        assert_eq!(result.first_msg, "fix the bug in auth");
        assert!(result.text.contains("also check the tests"));
    }

    #[test]
    fn test_tokenize_basic() {
        assert_eq!(tokenize("Hello World!"), vec!["hello", "world"]);
    }

    #[test]
    fn test_tokenize_underscores_and_accents() {
        let tokens = tokenize("my_var è pronto");
        assert_eq!(tokens, vec!["my_var", "è", "pronto"]);
    }

    #[test]
    fn test_tokenize_empty() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("!@#$%").is_empty());
    }

    #[test]
    fn test_bm25_relevant_doc_scores_higher() {
        let corpus = vec![
            "the cat sat on the mat".to_string(),
            "rust programming language systems".to_string(),
            "the dog played in the park".to_string(),
        ];
        let scores = score_bm25("rust programming", &corpus);
        assert!(scores[1] > scores[0]);
        assert!(scores[1] > scores[2]);
    }

    #[test]
    fn test_bm25_no_match_scores_zero() {
        let corpus = vec![
            "apple banana cherry".to_string(),
            "dog cat bird".to_string(),
        ];
        let scores = score_bm25("xyz123", &corpus);
        assert!(scores.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_format_result_line() {
        let line = format_result_line(
            1,
            0.847,
            "fix the authentication bug in the login flow",
            "~/projects/myapp",
            "abc12345-def6-7890-abcd-ef1234567890",
        );
        assert!(line.contains("1."));
        assert!(line.contains("[0.847]"));
        assert!(line.contains("fix the authentication"));
        assert!(line.contains("~/projects/myapp"));
        assert!(line.contains("abc12345..."));
    }

    #[test]
    fn test_cache_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_path = tmp.path().join("index.json");

        let mut cache = Cache::new(cache_path.clone());
        cache.entries.insert(
            "/fake/path.jsonl".to_string(),
            CacheEntry {
                mtime: 1234567890.0,
                text: "hello world".to_string(),
                cwd: "/tmp".to_string(),
                first_msg: "hello".to_string(),
            },
        );
        cache.save();

        let loaded = Cache::new(cache_path);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries["/fake/path.jsonl"].text, "hello world");
    }
}
