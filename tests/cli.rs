//! Runs the `npu-commit` binary end to end.

use std::path::Path;
use std::process::{Command, Output};

mod common;
use common::TestRepo;

fn npu_commit(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_npu-commit"))
        .args(args)
        .current_dir(dir)
        // The git the binary runs must not see the developer's config.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn unknown_command_prints_usage() {
    let repo = TestRepo::new();
    for args in [&[][..], &["frobnicate"], &["context", "a", "b"]] {
        let output = npu_commit(args, repo.path());
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).starts_with("usage:"), "{args:?}");
    }
}

#[test]
fn context_outside_a_repository_fails() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = npu_commit(&["context"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("not a git repository"));
}

#[test]
fn context_reports_the_detected_style() {
    let repo = TestRepo::new();
    repo.git(&["checkout", "--quiet", "-b", "feature/NPU-7-x"]);
    repo.commit("feat(cli): adiciona o comando context");
    repo.commit("fix(git): corrige a leitura da branch");

    let output = npu_commit(&["context"], repo.path());
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("branch:      feature/NPU-7-x"));
    assert!(out.contains("ticket:      NPU-7"));
    assert!(out.contains("convention:  Conventional (from 2 commits)"));
    assert!(out.contains("language:    Portuguese"));
    assert!(out.contains("scopes:      cli (1), git (1)"));
}

#[test]
fn context_accepts_a_path() {
    let repo = TestRepo::new();
    let elsewhere = tempfile::TempDir::new().unwrap();
    let path = repo.path().to_str().unwrap();
    let output = npu_commit(&["context", path], elsewhere.path());
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn prompt_without_staged_changes_fails() {
    let repo = TestRepo::new();
    repo.commit("init");
    let output = npu_commit(&["prompt"], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "error: nothing staged\n");
}

#[test]
fn prompt_shows_style_and_staged_diff() {
    let repo = TestRepo::new();
    repo.commit("Add the parser for the diff");
    repo.commit("Fix the crash when the diff is empty");
    repo.write("main.rs", "fn main() {}\n");
    repo.git(&["add", "main.rs"]);

    let output = npu_commit(&["prompt"], repo.path());
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("Write in English"));
    assert!(out.contains("Fix the crash when the diff is empty"));
    assert!(out.contains("main.rs | +1 -0"));
    assert!(out.contains("+fn main() {}"));
    assert!(out.contains("of 1024 tokens ====="));
}
