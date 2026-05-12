use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process;

const USAGE: &str = "\
claudio mv: Move a folder while preserving Claude Code sessions.

Usage:
  claudio mv <from> <to>
";

fn get_claude_projects_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("projects")
}

fn path_to_project_dirname(path: &Path) -> String {
    path.to_string_lossy().replace('/', "-")
}

fn rewrite_cwd_in_jsonl(file: &Path, old_cwd: &str, new_cwd: &str) -> bool {
    let Ok(f) = fs::File::open(file) else {
        return false;
    };
    let reader = BufReader::new(f);
    let mut lines = Vec::new();
    let mut changed = false;

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        if line.contains(old_cwd) {
            lines.push(line.replace(old_cwd, new_cwd));
            changed = true;
        } else {
            lines.push(line);
        }
    }

    if changed {
        let mut out = match fs::File::create(file) {
            Ok(f) => f,
            Err(_) => return false,
        };
        for line in &lines {
            let _ = writeln!(out, "{line}");
        }
    }

    changed
}

fn rewrite_sessions(project_dir: &Path, old_cwd: &str, new_cwd: &str) {
    let Ok(entries) = fs::read_dir(project_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rewrite_sessions(&path, old_cwd, new_cwd);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            rewrite_cwd_in_jsonl(&path, old_cwd, new_cwd);
        }
    }
}

pub fn run(args: &[String]) {
    if args.len() != 2 || args[0] == "-h" || args[0] == "--help" {
        eprint!("{USAGE}");
        process::exit(if args.len() != 2 { 1 } else { 0 });
    }

    let from = fs::canonicalize(&args[0]).unwrap_or_else(|e| {
        eprintln!("Error: cannot resolve '{}': {e}", args[0]);
        process::exit(1);
    });

    if !from.is_dir() {
        eprintln!("Error: '{}' is not a directory.", from.display());
        process::exit(1);
    }

    let to = PathBuf::from(&args[1]);
    if to.exists() {
        eprintln!("Error: '{}' already exists.", to.display());
        process::exit(1);
    }

    // Move the actual folder
    if let Err(e) = fs::rename(&from, &to) {
        eprintln!("Error moving folder: {e}");
        process::exit(1);
    }

    let to = fs::canonicalize(&to).unwrap_or_else(|e| {
        eprintln!("Error: cannot resolve '{}': {e}", args[1]);
        process::exit(1);
    });

    let from_str = from.to_string_lossy();
    let to_str = to.to_string_lossy();

    eprintln!("Moved {} -> {}", from_str, to_str);

    // Rename the Claude project directory
    let projects_dir = get_claude_projects_dir();
    let old_project_dir = projects_dir.join(path_to_project_dirname(&from));
    let new_project_dir = projects_dir.join(path_to_project_dirname(&to));

    if !old_project_dir.exists() {
        eprintln!("No Claude sessions found for this folder.");
        return;
    }

    if let Err(e) = fs::rename(&old_project_dir, &new_project_dir) {
        eprintln!("Error renaming project dir: {e}");
        process::exit(1);
    }

    // Rewrite cwd in all session files
    rewrite_sessions(&new_project_dir, &from_str, &to_str);

    eprintln!("Updated Claude sessions.");
}
