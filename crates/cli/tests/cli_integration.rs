use std::process::Command;

fn heisensim() -> Command {
    Command::new(env!("CARGO_BIN_EXE_heisensim"))
}

#[test]
fn test_help() {
    let output = heisensim().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("chaos"));
}

#[test]
fn test_version() {
    let output = heisensim().arg("--version").output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_run_help() {
    let output = heisensim().args(["run", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--namespace") || stdout.contains("--duration"));
}

#[test]
fn test_explore_help() {
    let output = heisensim().args(["explore", "--help"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_init_help() {
    let output = heisensim().args(["init", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_report_help() {
    let output = heisensim().args(["report", "--help"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_replay_help() {
    let output = heisensim().args(["replay", "--help"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_nonexistent_command() {
    let output = heisensim().arg("nonexistent-command").output().unwrap();
    assert!(!output.status.success());
}
