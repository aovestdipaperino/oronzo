use std::fs;
use std::io::{BufRead, BufReader};
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

fn session_id_from_file(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let obj: serde_json::Value = serde_json::from_str(&line).ok()?;
        if let Some(id) = obj.get("sessionId").and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
        return None;
    }
    None
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
                let id = session_id_from_file(&path).unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string()
                });
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
