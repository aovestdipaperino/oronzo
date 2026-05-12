use std::process::Command;

#[test]
fn test_help_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_claudio"))
        .arg("--help")
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("claudio"));
    assert!(stderr.contains("Usage"));
}

#[test]
fn test_no_args_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_claudio"))
        .output()
        .expect("failed to run binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage"));
}
