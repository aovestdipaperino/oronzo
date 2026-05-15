use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mdexport/proj_a")
}

#[test]
fn export_sess_with_tools_to_markdown() {
    use oronzo::mdexport::{parse_session, render, Args};
    let session = parse_session(&fixtures().join("sess_with_tools.jsonl")).unwrap();
    let md = render(&session, &Args::default());

    // Header
    assert!(md.contains("# Session 11111111-1111-1111-1111-111111111111"));
    assert!(md.contains("| Project | /tmp/proj_a |"));
    assert!(md.contains("| Git branch | main |"));
    // User turn
    assert!(md.contains("list the files"));
    // Bash tool rendering
    assert!(md.contains("_List directory_"));
    assert!(md.contains("```bash\nls -la\n```"));
    // tool_result
    assert!(md.contains("```text\ntotal 0"));
    // TodoWrite checklist
    assert!(md.contains("- [ ] check tests"));
    assert!(md.contains("- [ ] 🚧 review code"));
    assert!(md.contains("- [x] commit"));
}

#[test]
fn export_meta_only_session_yields_header_only() {
    use oronzo::mdexport::{parse_session, render, Args};
    let session = parse_session(&fixtures().join("sess_meta_only.jsonl")).unwrap();
    let md = render(&session, &Args::default());
    assert!(md.contains("# Session 66666666"));
    assert!(!md.contains("## User"));
    assert!(!md.contains("## Assistant"));
}

#[test]
fn flag_strips_tools_and_thinking() {
    use oronzo::mdexport::{parse_session, render, Args};
    let session = parse_session(&fixtures().join("sess_with_thinking.jsonl")).unwrap();
    let mut args = Args::default();
    args.thinking = false;
    let md = render(&session, &args);
    assert!(!md.contains("💭 Thinking"));
    assert!(md.contains("Here is X."));
}
