//! Runs the `npu-commit` binary end to end.

use std::path::Path;
use std::process::{Command, Output};

mod common;
use common::TestRepo;

fn npu_commit(args: &[&str], dir: &Path) -> Output {
    npu_commit_with(args, dir, &[])
}

fn npu_commit_with(args: &[&str], dir: &Path, env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_npu-commit"))
        .args(args)
        .current_dir(dir)
        // The git the binary runs must not see the developer's config.
        .envs(common::GIT_ENV.iter().copied())
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

/// A repository with a conventional history and one staged file.
fn staged_repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.commit("feat(git): add history");
    repo.commit("fix(git): handle detached HEAD");
    repo.write("main.rs", "fn main() {}\n");
    repo.git(&["add", "main.rs"]);
    repo
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn invalid_arguments_are_usage_errors() {
    let repo = TestRepo::new();
    for args in [
        &["frobnicate"][..],
        &["context", "a", "b"],
        &["--backend", "gpu"],
        &["context", "--commit"],
    ] {
        let output = npu_commit(args, repo.path());
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).starts_with("error:"), "{args:?}");
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

#[test]
fn generate_prints_the_sanitized_reply() {
    let repo = staged_repo();
    let reply = "Here is the commit message:\n```\nFeat(cli): add the main entry point.\n```";
    let output = npu_commit(
        &["--backend", "mock", "--mock-response", reply],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "feat(cli): add the main entry point\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn generate_retries_then_falls_back_with_a_warning() {
    let repo = staged_repo();
    let output = npu_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "Add the entry point",
        ],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "chore: add main.rs\n");
    assert!(stderr(&output).contains("unusable after 2 attempts"));
}

#[test]
fn generate_uses_the_second_reply_when_the_first_is_rejected() {
    let repo = staged_repo();
    let output = npu_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "Add the entry point",
            "--mock-response",
            "feat: add the entry point",
        ],
        repo.path(),
    );
    assert_eq!(stdout(&output), "feat: add the entry point\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn generate_without_a_backend_fails_clearly() {
    let repo = staged_repo();
    let output = npu_commit(&[], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("NPU backend is not available yet"));
}

#[test]
fn generate_with_nothing_staged_fails() {
    let repo = TestRepo::new();
    repo.commit("init");
    let output = npu_commit(&["--backend", "mock"], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "error: nothing staged\n");
}

#[test]
fn generate_accepts_a_path() {
    let repo = staged_repo();
    let elsewhere = tempfile::TempDir::new().unwrap();
    let path = repo.path().to_str().unwrap();
    let output = npu_commit(
        &[
            "-C",
            path,
            "--backend",
            "mock",
            "--mock-response",
            "feat: x y",
        ],
        elsewhere.path(),
    );
    assert_eq!(stdout(&output), "feat: x y\n");
}

#[test]
fn commit_goes_through_the_editor() {
    let repo = staged_repo();
    // `true` accepts the pre-filled message unchanged.
    let output = npu_commit_with(
        &[
            "--commit",
            "--backend",
            "mock",
            "--mock-response",
            "feat(cli): add the entry point\n\nSo the binary can run.",
        ],
        repo.path(),
        &[("GIT_EDITOR", "true")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        repo.git_output(&["log", "-1", "--format=%B"]),
        "feat(cli): add the entry point\n\nSo the binary can run.\n\n"
    );
    assert_eq!(repo.git_output(&["diff", "--staged", "--name-only"]), "");
}

#[test]
fn failed_editor_aborts_the_commit() {
    let repo = staged_repo();
    let output = npu_commit_with(
        &[
            "--commit",
            "--backend",
            "mock",
            "--mock-response",
            "feat: x y",
        ],
        repo.path(),
        &[("GIT_EDITOR", "false")],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("`git commit --edit` exited with"));
    assert_eq!(repo.git_output(&["rev-list", "--count", "HEAD"]), "2\n");
}
