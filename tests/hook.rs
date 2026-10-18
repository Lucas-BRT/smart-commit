//! Installs the `prepare-commit-msg` hook and commits through real git.

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use npu_commit::trace::Trace;

mod common;
use common::{TestRepo, npu_commit};

const MOCK: (&str, &str) = ("NPU_COMMIT_BACKEND", "mock");

fn reply(text: &str) -> (&str, &str) {
    ("NPU_COMMIT_MOCK_RESPONSE", text)
}

/// Accepts whatever message the editor was given.
const ACCEPT: (&str, &str) = ("GIT_EDITOR", "true");

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn hook_path(repo: &TestRepo) -> PathBuf {
    repo.path().join(".git/hooks/prepare-commit-msg")
}

/// A repository with a conventional history, the hook installed and one
/// staged file.
fn repo_with_hook() -> TestRepo {
    let repo = TestRepo::new();
    repo.commit("feat(git): add history");
    repo.commit("fix(git): handle detached HEAD");
    install(&repo);
    repo.write("main.rs", "fn main() {}\n");
    repo.git(&["add", "main.rs"]);
    repo
}

fn install(repo: &TestRepo) -> Output {
    let output = npu_commit(&["hook", "install"], repo.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    output
}

/// The full message of HEAD, without the newlines `%B` adds at the end.
fn last_message(repo: &TestRepo) -> String {
    repo.git_output(&["log", "-1", "--format=%B"])
        .trim_end()
        .to_owned()
}

#[test]
fn install_writes_an_executable_hook_calling_this_binary() {
    let repo = TestRepo::new();
    let output = install(&repo);
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("installed "));

    let script = fs::read_to_string(hook_path(&repo)).unwrap();
    assert!(npu_commit::hook::is_ours(&script));
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_npu-commit"))
        .canonicalize()
        .unwrap();
    assert!(script.contains(&format!("'{}' hook run", binary.display())));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(hook_path(&repo)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    // Installing again just updates our own hook.
    install(&repo);
}

#[test]
fn plain_commit_gets_the_generated_message() {
    let repo = repo_with_hook();
    let output = repo.git_with(
        &["commit", "--quiet"],
        &[
            MOCK,
            reply("feat(cli): add the entry point\n\nSo it runs."),
            ACCEPT,
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        last_message(&repo),
        "feat(cli): add the entry point\n\nSo it runs."
    );
}

#[test]
fn message_from_the_command_line_is_kept() {
    let repo = repo_with_hook();
    let output = repo.git_with(
        &["commit", "--quiet", "-m", "my own words"],
        &[MOCK, reply("feat: generated")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(last_message(&repo), "my own words");
}

#[test]
fn amend_keeps_the_previous_message() {
    let repo = repo_with_hook();
    repo.git(&["commit", "--quiet", "-m", "feat: first version"]);
    repo.write("main.rs", "fn main() { println!(); }\n");
    repo.git(&["add", "main.rs"]);
    let output = repo.git_with(
        &["commit", "--quiet", "--amend"],
        &[MOCK, reply("feat: generated"), ACCEPT],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(last_message(&repo), "feat: first version");
}

#[test]
fn merge_keeps_the_merge_message() {
    let repo = repo_with_hook();
    repo.git(&["commit", "--quiet", "-m", "feat: on main"]);
    repo.git(&["checkout", "--quiet", "-b", "topic"]);
    repo.commit("feat: on topic");
    repo.git(&["checkout", "--quiet", "main"]);
    repo.commit("fix: also on main");
    let output = repo.git_with(
        &["merge", "--quiet", "--no-ff", "--edit", "topic"],
        &[MOCK, reply("feat: generated"), ACCEPT],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(last_message(&repo), "Merge branch 'topic'");
}

#[test]
fn commit_all_sees_the_index_being_committed() {
    let repo = repo_with_hook();
    repo.git(&["commit", "--quiet", "-m", "feat: add main"]);
    // Not staged: only `commit -a` puts it in the (temporary) index.
    repo.write("main.rs", "fn main() { println!(); }\n");
    let output = repo.git_with(
        &["commit", "--quiet", "-a"],
        // An empty reply twice: the fallback names the files it saw.
        &[MOCK, reply(""), ACCEPT],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(last_message(&repo), "chore: update main.rs");
    assert!(stderr(&output).contains("unusable after 2 attempts"));
}

#[test]
fn failures_warn_but_never_block_the_commit() {
    let repo = repo_with_hook();
    // No model exported; the user writes the message instead.
    let output = repo.git_with(
        &["commit", "--quiet"],
        // Portable editor: git appends the file name, which becomes $1.
        &[("GIT_EDITOR", "sh -c 'echo \"written by hand\" > \"$1\"' -")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("npu-commit: backend unavailable: no model in"),
        "{}",
        stderr(&output)
    );
    assert_eq!(last_message(&repo), "written by hand");
}

#[test]
fn hook_can_record_a_trace_that_replays() {
    let repo = repo_with_hook();
    let trace = repo.path().join("hook-trace.json");
    let output = repo.git_with(
        &["commit", "--quiet"],
        &[
            MOCK,
            reply("Add the entry point"),
            ("NPU_COMMIT_TRACE", trace.to_str().unwrap()),
            ACCEPT,
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let trace = Trace::from_json(&fs::read_to_string(&trace).unwrap()).unwrap();
    assert_eq!(trace.inputs.files[0].path, "main.rs");
    assert!(trace.replay().unwrap().differences.is_empty());
}

#[test]
fn existing_hooks_are_protected() {
    let repo = TestRepo::new();
    let foreign = "#!/bin/sh\nexit 0\n";
    fs::create_dir_all(hook_path(&repo).parent().unwrap()).unwrap();
    fs::write(hook_path(&repo), foreign).unwrap();

    let output = npu_commit(&["hook", "install"], repo.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("use --force"));
    let output = npu_commit(&["hook", "uninstall"], repo.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("leaving it alone"));
    assert_eq!(fs::read_to_string(hook_path(&repo)).unwrap(), foreign);

    // --force keeps the original as a backup, and uninstall restores it.
    let output = npu_commit(&["hook", "install", "--force"], repo.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(npu_commit::hook::is_ours(
        &fs::read_to_string(hook_path(&repo)).unwrap()
    ));
    let output = npu_commit(&["hook", "uninstall"], repo.path(), &[]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(fs::read_to_string(hook_path(&repo)).unwrap(), foreign);
}

#[test]
fn force_never_overwrites_an_earlier_backup() {
    let repo = TestRepo::new();
    let hooks = hook_path(&repo).parent().unwrap().to_path_buf();
    fs::create_dir_all(&hooks).unwrap();
    fs::write(hook_path(&repo), "#!/bin/sh\necho first\n").unwrap();
    fs::write(hooks.join("prepare-commit-msg.npu-commit-backup"), "older").unwrap();

    let output = npu_commit(&["hook", "install", "--force"], repo.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(hooks.join("prepare-commit-msg.npu-commit-backup")).unwrap(),
        "older"
    );
}

#[test]
fn uninstall_without_a_hook_fails() {
    let repo = TestRepo::new();
    let output = npu_commit(&["hook", "uninstall"], repo.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("no prepare-commit-msg hook"));
}

#[test]
fn install_honours_core_hooks_path() {
    let repo = TestRepo::new();
    repo.git(&["config", "core.hooksPath", "tools/hooks"]);
    install(&repo);
    let script = fs::read_to_string(repo.path().join("tools/hooks/prepare-commit-msg")).unwrap();
    assert!(npu_commit::hook::is_ours(&script));
    assert!(!hook_path(&repo).exists());
}
