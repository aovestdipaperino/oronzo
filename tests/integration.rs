use std::process::Command;

#[test]
fn test_help_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_claudio"))
        .arg("--help")
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("claudio"));
    assert!(stderr.contains("Commands:"));
}

#[test]
fn test_no_args_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_claudio"))
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Commands:"));
}

#[test]
fn test_unknown_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_claudio"))
        .arg("bogus")
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown command: bogus"));
    assert!(!output.status.success());
}

#[test]
fn test_search_no_query_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_claudio"))
        .arg("search")
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("claudio search"));
}
