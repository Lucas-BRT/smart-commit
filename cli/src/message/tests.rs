use super::*;

use crate::git::{ChangeStatus, FileChange};
use crate::style::Convention;

fn clean(raw: &str) -> String {
    sanitize(raw, Convention::Free).unwrap().to_string()
}

fn conventional(raw: &str) -> Result<String, SanitizeError> {
    sanitize(raw, Convention::Conventional).map(|m| m.to_string())
}

fn change(status: ChangeStatus, path: &str) -> FileChange {
    FileChange {
        status,
        path: path.to_owned(),
        old_path: None,
    }
}

#[test]
fn strips_fences_quotes_and_preambles() {
    for raw in [
        "```\nAdd login page\n```",
        "```text\nAdd login page\n```\n",
        "\"Add login page\"",
        "`Add login page`",
        "“Add login page”",
        "**Add login page**",
        "Here is the commit message:\n\nAdd login page",
        "Sure! Here's a commit message:\n```\nAdd login page\n```",
        "Commit message: Add login page",
        "**Commit message:** Add login page",
        "Mensagem de commit:\nAdd login page",
        "# Add login page",
        "- Add login page",
        "1. Add login page",
        "Add login page.",
        "  Add   login page  ",
    ] {
        assert_eq!(clean(raw), "Add login page", "{raw:?}");
    }
}

#[test]
fn drops_thinking_blocks() {
    assert_eq!(
        clean("<think>\nThe diff adds a page.\n</think>\n\nAdd login page"),
        "Add login page"
    );
    assert_eq!(
        sanitize("<think>still thinking", Convention::Free),
        Err(SanitizeError::Empty)
    );
}

#[test]
fn drops_every_thinking_block() {
    assert_eq!(
        clean("<think>a</think>Add login<think>b</think> page"),
        "Add login page"
    );
}

#[test]
fn empty_fence_keeps_the_text_around_it() {
    assert_eq!(clean("Add login page\n```\n```"), "Add login page");
}

#[test]
fn only_the_first_fence_is_used() {
    assert_eq!(
        clean("```\nAdd login page\n```\n\nor:\n```\nCreate login page\n```"),
        "Add login page"
    );
}

#[test]
fn drops_explanations_after_the_message() {
    assert_eq!(
        clean("Add login page\n\nThis commit message describes the new page."),
        "Add login page"
    );
    assert_eq!(
        clean("Adiciona login\n\nPermite entrar.\n\n**Explicação:** segui o padrão."),
        "Adiciona login\n\nPermite entrar."
    );
}

#[test]
fn empty_output_is_an_error() {
    for raw in ["", "   \n\n", "```\n```", "\"\"", "..."] {
        assert_eq!(
            sanitize(raw, Convention::Free),
            Err(SanitizeError::Empty),
            "{raw:?}"
        );
    }
}

#[test]
fn fixes_near_conventional_subjects() {
    assert_eq!(conventional("Feat: add login").unwrap(), "feat: add login");
    assert_eq!(conventional("feat:add login").unwrap(), "feat: add login");
    assert_eq!(
        conventional("Fix(api)!: drop v1").unwrap(),
        "fix(api)!: drop v1"
    );
    assert_eq!(
        conventional("```\nfeat(cli): add flag\n```").unwrap(),
        "feat(cli): add flag"
    );
}

#[test]
fn rejects_non_conventional_subjects() {
    for raw in ["Add login page", "Here is: the message", "feat(: x"] {
        assert!(
            matches!(conventional(raw), Err(SanitizeError::NotConventional(_))),
            "{raw:?}"
        );
    }
}

#[test]
fn long_subject_is_cut_at_a_word() {
    let raw = "feat(parser): support renames, copies and type changes in the staged diff output";
    let subject = conventional(raw).unwrap();
    // At the phrase "in ...", not mid-phrase ("... in the staged").
    assert_eq!(
        subject,
        "feat(parser): support renames, copies and type changes"
    );
    assert!(subject.chars().count() <= SUBJECT_MAX);

    let unbroken = "x".repeat(100);
    assert_eq!(clean(&unbroken).chars().count(), SUBJECT_MAX);
    let accented = format!("Atualiza {}", "ç".repeat(100));
    assert_eq!(clean(&accented), "Atualiza");
}

#[test]
fn the_limit_is_the_configured_one() {
    let raw = "fix(kubernetes): update manifest and pod handlers to handle job and pod conditions";
    // 82 columns fit in 100; in 60 the subject ends at "to handle".
    let wide = sanitize_within(raw, Convention::Conventional, 100).unwrap();
    assert_eq!(wide.subject, raw);
    let narrow = sanitize_within(raw, Convention::Conventional, 60).unwrap();
    assert_eq!(
        narrow.subject,
        "fix(kubernetes): update manifest and pod handlers"
    );
    let changes = [change(
        ChangeStatus::Modified,
        "src/a/very/long/path/to/some/module/file.rs",
    )];
    let fallback = fallback_after(&changes, Convention::Conventional, None, 40);
    assert!(
        fallback.subject.chars().count() <= 40,
        "{}",
        fallback.subject
    );
}

#[test]
fn long_subjects_end_where_they_still_read() {
    // Replies of qwen2.5-coder-1.5b on eval-v1, over the limit.
    for (raw, subject) in [
        (
            "fix(kubernetes): update manifest and pod handlers to handle job and pod conditions",
            "fix(kubernetes): update manifest and pod handlers",
        ),
        (
            "fix(clouddriver-core): update RetrofitConfig to support capacity provider strategies",
            "fix(clouddriver-core): update RetrofitConfig",
        ),
        (
            "fix(src/operator/combineLatest.ts): update slice method to avoid unnecessary copying",
            "fix(combineLatest): update slice method to avoid unnecessary copying",
        ),
        (
            "fix(doc/lettable-operators.md): update doc on building with the rxjs/operators import location",
            "fix(lettable-operators): update doc on building",
        ),
        (
            "fix(provider/kubernetes): Update cluster provider logic to handle missing providers",
            "fix(provider/kubernetes): Update cluster provider logic",
        ),
        (
            "refactor(src/internal/Subscriber.ts, src/internal/scheduler/AnimationFrameAction.ts): use the new names",
            "refactor(Subscriber): use the new names",
        ),
    ] {
        assert_eq!(conventional(raw).unwrap(), subject, "{raw}");
    }
    // A path scope is kept when the subject fits: only length shortens it.
    assert_eq!(
        conventional("fix(src/lib.rs): handle empty input").unwrap(),
        "fix(src/lib.rs): handle empty input"
    );
    // A boundary that would keep too little falls back to the last word.
    let raw = "fix: crash, the parser reads the whole staged diff output twice over every time";
    let subject = conventional(raw).unwrap();
    assert!(
        subject.starts_with("fix: crash, the parser reads"),
        "{subject}"
    );
    assert!(subject.chars().count() <= SUBJECT_MAX);
}

#[test]
fn body_is_rewrapped() {
    let raw = "Add login page\n\nThis lets users sign in with their email address and a password, \
               which was the most requested feature.\n\n* first point\n* second point that is long \
               enough to need wrapping onto a second line of the body";
    insta::assert_snapshot!(clean(raw), @r"
    Add login page

    This lets users sign in with their email address and a password, which
    was the most requested feature.

    - first point
    - second point that is long enough to need wrapping onto a second line
      of the body
    ");
}

#[test]
fn fallback_after_a_rejected_reply() {
    let code = [change(ChangeStatus::Modified, "src/git.rs")];
    let docs = [change(ChangeStatus::Modified, "AGENTS.md")];
    let after = |changes: &[FileChange], reply| {
        fallback_after(changes, Convention::Conventional, reply, SUBJECT_MAX).subject
    };
    // Code: the reply's type and scope beat the file list's `chore`.
    assert_eq!(
        after(&code, Some("fix(git): handle detached HEAD")),
        "fix(git): update src/git.rs"
    );
    assert_eq!(after(&code, Some("feat: add x")), "feat: update src/git.rs");
    // Docs only: the files tell the type; the scope is still kept
    // (fixture conventional-en-partial-copy).
    assert_eq!(
        after(&docs, Some("fix(agents): trace a docs-only change")),
        "docs(agents): update AGENTS.md"
    );
    // No reply, or one without a header: as before.
    assert_eq!(after(&code, None), "chore: update src/git.rs");
    assert_eq!(
        after(&code, Some("Handle detached HEAD")),
        "chore: update src/git.rs"
    );
    // Free style has no type to keep.
    assert_eq!(
        fallback_after(&code, Convention::Free, Some("fix(git): x"), SUBJECT_MAX,).subject,
        "Update src/git.rs"
    );
}

#[test]
fn fallback_for_a_single_file() {
    let added = [change(ChangeStatus::Added, "src/diff.rs")];
    assert_eq!(
        fallback(&added, Convention::Conventional).to_string(),
        "chore: add src/diff.rs"
    );
    assert_eq!(
        fallback(&added, Convention::Free).to_string(),
        "Add src/diff.rs"
    );
    let renamed = [FileChange {
        old_path: Some("a.md".into()),
        ..change(ChangeStatus::Renamed, "docs/b.md")
    }];
    assert_eq!(
        fallback(&renamed, Convention::Gitmoji).to_string(),
        "📝 rename a.md to docs/b.md"
    );
}

#[test]
fn fallback_for_many_files_lists_them() {
    let changes: Vec<FileChange> = (0..12)
        .map(|n| change(ChangeStatus::Modified, &format!("tests/case_{n}.rs")))
        .collect();
    let message = fallback(&changes, Convention::Conventional);
    assert_eq!(message.subject, "test: update 12 files in tests/");
    let body = message.body.unwrap();
    assert_eq!(body.lines().count(), 11);
    assert!(body.ends_with("- ... and 2 more"));
}

#[test]
fn fallback_picks_type_and_emoji_from_the_files() {
    let config = [
        change(ChangeStatus::Modified, "Cargo.toml"),
        change(ChangeStatus::Modified, "Cargo.lock"),
    ];
    assert_eq!(
        fallback(&config, Convention::Gitmoji).subject,
        "🔧 update 2 files"
    );
    assert_eq!(
        fallback(&config, Convention::Conventional).subject,
        "chore: update 2 files"
    );
    let mixed = [
        change(ChangeStatus::Modified, "src/a.rs"),
        change(ChangeStatus::Deleted, "docs/b.md"),
    ];
    assert_eq!(
        fallback(&mixed, Convention::Gitmoji).subject,
        "🚧 update 2 files"
    );
    assert_eq!(
        fallback(&mixed, Convention::Free).to_string(),
        "Update 2 files\n\n- src/a.rs\n- docs/b.md"
    );
    let deleted = [change(ChangeStatus::Deleted, "docs/old.md")];
    assert_eq!(
        fallback(&deleted, Convention::Unknown).subject,
        "docs: remove docs/old.md"
    );
}

#[test]
fn fallback_subject_fits_72_columns() {
    let path = format!("src/{}/mod.rs", "deeply_nested".repeat(8));
    let message = fallback(
        &[change(ChangeStatus::Added, &path)],
        Convention::Conventional,
    );
    assert!(message.subject.chars().count() <= SUBJECT_MAX);
}

#[test]
fn common_dir_is_the_shared_prefix() {
    let at = |paths: &[&str]| {
        fallback::common_dir(
            &paths
                .iter()
                .map(|p| change(ChangeStatus::Modified, p))
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(at(&["src/a/x.rs", "src/a/y.rs"]).as_deref(), Some("src/a/"));
    assert_eq!(at(&["src/a/x.rs", "src/b.rs"]).as_deref(), Some("src/"));
    assert_eq!(at(&["src/x.rs", "README.md"]), None);
}
