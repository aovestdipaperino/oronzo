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
