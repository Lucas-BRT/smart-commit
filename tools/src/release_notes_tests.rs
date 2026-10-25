use std::path::Path;
use std::process::Command;

use super::*;

fn subjects(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn subjects_are_grouped_by_type_and_the_scope_is_bold() {
    let notes = render(
        "v0.2.0",
        Some("v0.1.0"),
        &subjects(&[
            "feat(cli): adiciona o comando bench",
            "fix: corrige o parse",
            "docs: atualiza o guia",
            "perf(diff)!: corta menos",
        ]),
    );
    assert!(notes.starts_with("## smart-commit 0.2.0\n\nChanges since v0.1.0.\n\n"));
    assert!(notes.contains("### Features\n\n- **cli** adiciona o comando bench\n\n"));
    assert!(notes.contains("### Fixes\n\n- corrige o parse\n\n"));
    assert!(notes.contains("### Performance\n\n- **diff** corta menos\n\n"));
    assert!(notes.contains("### Documentation\n\n- atualiza o guia\n\n"));
    assert!(!notes.contains("- **** "));
    assert!(!notes.contains("### Other"));
}

#[test]
fn other_types_and_free_text_go_under_other() {
    let notes = render(
        "v0.2.0",
        Some("v0.1.0"),
        &subjects(&[
            "build: fixa o toolchain",
            "chore(deps): sobe o clap",
            "Mensagem livre",
            "feat",
        ]),
    );
    assert!(notes.contains(
        "### Other\n\n- fixa o toolchain\n- **deps** sobe o clap\n- Mensagem livre\n- feat\n\n"
    ));
    assert!(!notes.contains("### Features"));
}

#[test]
fn what_looks_like_a_type_but_is_not_one_is_free_text() {
    assert_eq!(conventional("Feat: x"), None);
    assert_eq!(conventional("feat(a)b: x"), None);
    assert_eq!(conventional(": x"), None);
    assert_eq!(conventional("feat(a: b)"), None);
    assert_eq!(conventional("fix(a)!: x"), Some(("fix", "a", "x")));
}

#[test]
fn the_first_release_says_so_and_every_release_says_how_to_install() {
    let notes = render("v0.1.0", None, &[]);
    assert!(notes.contains("First release.\n"));
    assert!(notes.contains("install.sh | bash"));
    assert!(notes.ends_with("on any machine with Docker.\n"));
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn only_the_commits_since_the_previous_tag_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "--initial-branch=main"]);
    let commit = |subject: &str| git(repo, &["commit", "-q", "--allow-empty", "-m", subject]);
    commit("feat: before the tag");
    git(repo, &["tag", "v0.1.0"]);
    commit("feat(cli): adiciona o comando bench");
    commit("fix: corrige o parse");
    commit("Mensagem livre");

    let text = notes(repo, "v0.2.0", "HEAD").unwrap();
    assert!(text.contains("Changes since v0.1.0."));
    assert!(text.contains("- **cli** adiciona o comando bench"));
    assert!(text.contains("- Mensagem livre"));
    assert!(!text.contains("before the tag"));

    let first = notes(repo, "v0.1.0", "v0.1.0").unwrap();
    assert!(first.contains("First release."));
    assert!(first.contains("- before the tag"));
}

#[test]
fn merges_are_left_out() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "--initial-branch=main"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "feat: base"]);
    git(repo, &["tag", "v0.1.0"]);
    git(repo, &["switch", "-q", "-c", "side"]);
    git(
        repo,
        &["commit", "-q", "--allow-empty", "-m", "fix: on the side"],
    );
    git(repo, &["switch", "-q", "main"]);
    git(
        repo,
        &["commit", "-q", "--allow-empty", "-m", "feat: on main"],
    );
    git(
        repo,
        &["merge", "-q", "--no-ff", "-m", "Merge branch side", "side"],
    );
    let text = notes(repo, "v0.2.0", "HEAD").unwrap();
    assert!(text.contains("on the side") && text.contains("on main"));
    assert!(!text.contains("Merge branch"));
}
