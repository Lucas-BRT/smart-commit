//! Runs the context collection against real, throwaway git repositories.

use std::fs;

use smart_commit::context::RepoContext;
use smart_commit::diff;
use smart_commit::git::{ChangeStatus, FileChange, GitError, Repo};
use smart_commit::prompt::{self, PromptOptions};
use smart_commit::style::{Convention, Language};
use tempfile::TempDir;

mod common;
use common::TestRepo;

#[test]
fn outside_a_repository_is_an_error() {
    let dir = TempDir::new().unwrap();
    assert!(matches!(
        Repo::discover(dir.path()),
        Err(GitError::NotARepository(_))
    ));
}

#[test]
fn discovers_root_from_subdirectory() {
    let repo = TestRepo::new();
    let sub = repo.path().join("a/b");
    fs::create_dir_all(&sub).unwrap();

    let found = Repo::discover(&sub).unwrap();
    assert_eq!(
        found.root().canonicalize().unwrap(),
        repo.path().canonicalize().unwrap()
    );
}

#[test]
fn fresh_repository_has_no_style() {
    let repo = TestRepo::new();
    let ctx = repo.context();
    assert_eq!(ctx.branch.as_deref(), Some("main"));
    assert_eq!(ctx.style.convention, Convention::Unknown);
    assert!(ctx.staged.is_empty());
}

#[test]
fn staged_files_before_first_commit_are_added() {
    let repo = TestRepo::new();
    repo.write("README.md", "hello\n");
    repo.git(&["add", "README.md"]);

    assert_eq!(
        repo.context().staged,
        vec![FileChange {
            status: ChangeStatus::Added,
            path: "README.md".into(),
            old_path: None,
        }]
    );
}

#[test]
fn learns_style_from_history_most_recent_first() {
    let repo = TestRepo::new();
    for subject in [
        "chore: initial commit",
        "feat(git): coleta o histórico de commits",
        "fix(style): corrige a detecção de escopo",
        "feat(git): adiciona suporte para renomeação",
    ] {
        repo.commit(subject);
    }

    let style = repo.context().style;
    assert_eq!(style.convention, Convention::Conventional);
    assert_eq!(style.language, Language::Portuguese);
    assert_eq!(style.scopes[0], ("git".into(), 2));
    assert_eq!(
        style.examples[0],
        "feat(git): adiciona suporte para renomeação"
    );
    assert_eq!(style.sample_size, 4);
}

#[test]
fn merge_commits_are_ignored() {
    let repo = TestRepo::new();
    repo.commit("feat: base");
    repo.git(&["checkout", "--quiet", "-b", "topic"]);
    repo.commit("feat: topic work");
    repo.git(&["checkout", "--quiet", "main"]);
    repo.commit("fix: main work");
    repo.git(&[
        "merge",
        "--quiet",
        "--no-ff",
        "-m",
        "Merge branch 'topic'",
        "topic",
    ]);

    let style = repo.context().style;
    assert_eq!(style.sample_size, 3);
    assert!(style.examples.iter().all(|e| !e.starts_with("Merge")));
}

#[test]
fn reads_ticket_from_branch() {
    let repo = TestRepo::new();
    repo.commit("init");
    repo.git(&["checkout", "--quiet", "-b", "feature/NPU-42-hook"]);

    let ctx = repo.context();
    assert_eq!(ctx.branch.as_deref(), Some("feature/NPU-42-hook"));
    assert_eq!(ctx.ticket.as_deref(), Some("NPU-42"));
}

#[test]
fn detached_head_has_no_branch() {
    let repo = TestRepo::new();
    repo.commit("init");
    repo.git(&["checkout", "--quiet", "--detach"]);
    assert_eq!(repo.context().branch, None);
}

#[test]
fn reports_staged_changes_only() {
    let repo = TestRepo::new();
    let body = "line one\nline two\nline three\nline four\n";
    repo.write("keep.rs", "fn main() {}\n");
    repo.write("gone.rs", "fn gone() {}\n");
    repo.write("old_name.rs", body);
    repo.git(&["add", "."]);
    repo.commit("init");

    repo.write("keep.rs", "fn main() { println!(); }\n");
    repo.write("new.rs", "fn new() {}\n");
    repo.write("unstaged.rs", "not added\n");
    repo.git(&["rm", "--quiet", "gone.rs"]);
    repo.git(&["mv", "old_name.rs", "new_name.rs"]);
    repo.git(&["add", "keep.rs", "new.rs"]);

    let mut staged = repo.context().staged;
    staged.sort_by(|a, b| a.path.cmp(&b.path));
    let summary: Vec<(ChangeStatus, &str, Option<&str>)> = staged
        .iter()
        .map(|c| (c.status, c.path.as_str(), c.old_path.as_deref()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (ChangeStatus::Deleted, "gone.rs", None),
            (ChangeStatus::Modified, "keep.rs", None),
            (ChangeStatus::Added, "new.rs", None),
            (ChangeStatus::Renamed, "new_name.rs", Some("old_name.rs")),
        ]
    );
}

#[test]
fn staged_diff_pairs_hunks_with_files() {
    let repo = TestRepo::new();
    let body = "line one\nline two\nline three\nline four\n";
    repo.write("keep.rs", "fn main() {}\n");
    repo.write("old_name.rs", body);
    repo.git(&["add", "."]);
    repo.commit("init");

    repo.write("keep.rs", "fn main() { println!(); }\n");
    repo.write("with space.rs", "fn spaced() {}\n");
    fs::write(repo.path().join("logo.png"), [0u8, 159, 146, 150]).unwrap();
    repo.git(&["mv", "old_name.rs", "new_name.rs"]);
    repo.git(&["add", "."]);

    let files = repo.repo().staged_diff().unwrap();
    let by_path = |path: &str| files.iter().find(|f| f.path == path).unwrap();

    let keep = by_path("keep.rs");
    assert_eq!(keep.lines, Some((1, 1)));
    assert!(keep.hunks.starts_with("@@"));
    assert!(keep.hunks.contains("+fn main() { println!(); }"));

    assert!(by_path("with space.rs").hunks.contains("+fn spaced()"));
    assert_eq!(by_path("logo.png").lines, None);

    let renamed = by_path("new_name.rs");
    assert_eq!(renamed.old_path.as_deref(), Some("old_name.rs"));
    assert_eq!(renamed.hunks, "", "a pure rename has no hunks");
}

#[test]
fn staged_diff_before_first_commit() {
    let repo = TestRepo::new();
    repo.write("README.md", "hello\n");
    repo.git(&["add", "README.md"]);

    let files = repo.repo().staged_diff().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].lines, Some((1, 0)));
    assert!(files[0].hunks.contains("+hello"));
}

#[test]
fn prompt_from_a_real_repository_fits_the_budget() {
    let repo = TestRepo::new();
    repo.commit("feat(git): coleta o histórico de commits");
    repo.commit("fix(style): corrige a detecção de escopo");
    for n in 0..40 {
        let contents: String = (0..80).map(|l| format!("let v{l} = {n};\n")).collect();
        repo.write(&format!("m{n}.rs"), &contents);
    }
    repo.git(&["add", "."]);

    let ctx = repo.context();
    let files = repo.repo().staged_diff().unwrap();
    let options = PromptOptions::default();
    let prompt = prompt::build(&ctx, &files, &options);

    assert!(prompt.system.contains("Write in Brazilian Portuguese."));
    assert!(prompt.user.contains("m0.rs | +80 -0"));
    let used = diff::estimate_tokens(&prompt.system) + diff::estimate_tokens(&prompt.user);
    assert!(used < options.max_prompt_tokens, "{used}");
}

#[test]
fn staged_diff_ignores_color_external_tools_and_prefix_config() {
    let repo = TestRepo::new();
    repo.write("a.rs", "old\n");
    repo.git(&["add", "."]);
    repo.commit("init");
    repo.git(&["config", "color.ui", "always"]);
    repo.git(&["config", "color.diff", "always"]);
    repo.git(&["config", "diff.noprefix", "true"]);
    repo.git(&["config", "diff.external", "false"]);
    repo.write("a.rs", "new\n");
    repo.git(&["add", "a.rs"]);

    let files = repo.repo().staged_diff().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].hunks, "@@ -1 +1 @@\n-old\n+new\n");
}

#[test]
fn repo_environment_reaches_every_git_call() {
    let repo = TestRepo::new();
    repo.write("a.rs", "one\ntwo\nthree\n");
    repo.git(&["add", "."]);
    repo.commit("init");
    repo.git(&["mv", "a.rs", "b.rs"]);

    let config = repo.path().join("isolated.gitconfig");
    fs::write(&config, "[diff]\n\trenames = false\n").unwrap();
    let without_renames = repo
        .repo()
        .with_env([("GIT_CONFIG_GLOBAL", config.to_str().unwrap())]);

    let statuses: Vec<ChangeStatus> = without_renames
        .staged_changes()
        .unwrap()
        .iter()
        .map(|c| c.status)
        .collect();
    assert_eq!(statuses, [ChangeStatus::Deleted, ChangeStatus::Added]);
    assert_eq!(repo.context().staged[0].status, ChangeStatus::Renamed);
}

#[test]
fn hooks_dir_honours_core_hooks_path() {
    let repo = TestRepo::new();
    let default = repo.repo().hooks_dir().unwrap();
    assert_eq!(
        default.canonicalize().unwrap(),
        repo.path().join(".git/hooks").canonicalize().unwrap()
    );

    repo.git(&["config", "core.hooksPath", "tools/hooks"]);
    assert_eq!(
        repo.repo().hooks_dir().unwrap(),
        repo.repo().root().join("tools/hooks")
    );
    repo.git(&["config", "core.hooksPath", "/etc/shared-hooks"]);
    assert_eq!(
        repo.repo().hooks_dir().unwrap(),
        std::path::PathBuf::from("/etc/shared-hooks")
    );
}

#[test]
fn comment_prefix_defaults_to_hash() {
    let repo = TestRepo::new();
    assert_eq!(repo.repo().comment_prefix().unwrap(), "#");
    repo.git(&["config", "core.commentChar", ";"]);
    assert_eq!(repo.repo().comment_prefix().unwrap(), ";");
    repo.git(&["config", "core.commentChar", "auto"]);
    assert_eq!(repo.repo().comment_prefix().unwrap(), "#");
}

#[test]
fn history_depth_limits_the_subjects() {
    let repo = TestRepo::new();
    for n in 1..=5 {
        repo.commit(&format!("feat: change number {n}"));
    }
    let ctx = RepoContext::collect_with_history(&repo.repo(), 2).unwrap();
    assert_eq!(
        ctx.subjects,
        ["feat: change number 5", "feat: change number 4"]
    );
    assert_eq!(ctx.style.sample_size, 2);
}

#[test]
fn commits_what_is_staged_with_a_message() {
    let repo = TestRepo::new();
    repo.commit("feat: start");
    repo.write("b.rs", "fn b() {}\n");
    repo.git(&["add", "b.rs"]);
    repo.repo().commit("feat: add b\n\nWith a body.").unwrap();
    assert_eq!(
        repo.git_output(&["log", "-1", "--format=%B"]).trim_end(),
        "feat: add b\n\nWith a body."
    );
    assert_eq!(repo.git_output(&["status", "--porcelain"]), "");
    // Nothing staged: git refuses, and so does commit.
    assert!(matches!(
        repo.repo().commit("feat: nothing"),
        Err(GitError::Exited { .. })
    ));
}
