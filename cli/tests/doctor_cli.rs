//! `smart-commit doctor`, against installations built in temporary
//! directories: a fresh one, a complete one, inside and outside a
//! repository.

use std::fs;
use std::path::Path;
use std::process::Output;

use smart_commit::backend::llamacpp::LLAMA_CPP_BUILD;
use smart_commit::models;
use tempfile::TempDir;

mod common;
use common::{TestRepo, smart_commit};

struct Home {
    config: TempDir,
    data: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            config: TempDir::new().unwrap(),
            data: TempDir::new().unwrap(),
        }
    }

    fn doctor(&self, args: &[&str], dir: &Path) -> Output {
        let mut all = vec!["doctor"];
        all.extend_from_slice(args);
        smart_commit(
            &all,
            dir,
            &[
                ("XDG_CONFIG_HOME", self.config.path().to_str().unwrap()),
                ("XDG_DATA_HOME", self.data.path().to_str().unwrap()),
            ],
        )
    }

    /// What `smart-commit setup` leaves for the llama.cpp devices: the
    /// pinned GGUF (its size and record, not its content) and llama-server.
    fn install(&self) {
        let share = self.data.path().join("smart-commit");
        let model = models::find(models::DEFAULT).unwrap();
        fs::create_dir_all(share.join("models")).unwrap();
        let file = share.join("models").join(model.gguf.file);
        fs::File::create(&file)
            .unwrap()
            .set_len(model.gguf.size)
            .unwrap();
        fs::write(models::sidecar(&file), model.gguf.sha256).unwrap();
        let server = share.join(format!("llama.cpp/llama-{LLAMA_CPP_BUILD}"));
        fs::create_dir_all(&server).unwrap();
        fs::write(server.join("llama-server"), "").unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn line<'a>(report: &'a str, start: &str) -> &'a str {
    report
        .lines()
        .find(|l| l.starts_with(start))
        .unwrap_or_else(|| panic!("no `{start}` line in:\n{report}"))
}

#[test]
fn a_fresh_home_fails_for_the_missing_model() {
    let home = Home::new();
    let cwd = TempDir::new().unwrap();
    let output = home.doctor(&[], cwd.path());
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    let model = line(&report, "fail  model");
    assert!(model.contains("no GGUF model at"), "{report}");
    assert!(model.contains("smart-commit setup"), "{report}");
    assert!(line(&report, "warn  llama.cpp").contains("no llama-server"));
    assert!(line(&report, "warn  device").contains("no benchmark"));
    assert!(line(&report, "ok    config").contains("defaults"));
    // Outside a repository there is no hook to check.
    assert!(!report.contains("hook"), "{report}");
}

#[test]
fn an_installed_tree_passes_and_warnings_keep_the_exit_code() {
    let home = Home::new();
    home.install();
    let cwd = TempDir::new().unwrap();
    let output = home.doctor(&[], cwd.path());
    let report = stdout(&output);
    assert!(output.status.success(), "{report}");
    assert!(line(&report, "ok    model").contains("size and record match"));
    assert!(line(&report, "ok    llama.cpp").contains("llama-server"));
    // No benchmark yet: a warning, not a failure.
    assert!(line(&report, "warn  device").contains("smart-commit bench"));
    assert!(line(&report, "ok    server").contains("none running"));
    assert!(!report.contains("\nfail"), "{report}");
}

#[test]
fn a_model_of_the_wrong_size_fails() {
    let home = Home::new();
    home.install();
    let model = models::find(models::DEFAULT).unwrap();
    let file = home
        .data
        .path()
        .join("smart-commit/models")
        .join(model.gguf.file);
    fs::File::create(&file).unwrap().set_len(10).unwrap();
    let output = home.doctor(&[], home.data.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(line(&stdout(&output), "fail  model").contains("not the pinned"));
}

#[test]
fn a_config_that_does_not_parse_fails() {
    let home = Home::new();
    home.install();
    let dir = home.config.path().join("smart-commit");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("config.toml"), "[model\n").unwrap();
    let output = home.doctor(&[], home.data.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(line(&stdout(&output), "fail  config").contains("config.toml"));
}

#[test]
fn inside_a_repository_it_checks_the_hook() {
    let home = Home::new();
    home.install();
    let repo = TestRepo::new();
    let report = stdout(&home.doctor(&[], repo.path()));
    let hook = line(&report, "warn  hook");
    assert!(hook.contains("smart-commit hook install"), "{report}");

    let installed = smart_commit(&["hook", "install"], repo.path(), &[]);
    assert!(installed.status.success());
    let report = stdout(&home.doctor(&[], repo.path()));
    assert!(
        line(&report, "ok    hook").contains("runs this binary"),
        "{report}"
    );

    // The hook is ignored once git looks elsewhere.
    repo.git(&["config", "core.hooksPath", ".githooks"]);
    let report = stdout(&home.doctor(&[], repo.path()));
    assert!(
        line(&report, "warn  hook").contains("core.hooksPath"),
        "{report}"
    );
}

#[test]
fn a_repository_file_is_checked_too() {
    let home = Home::new();
    home.install();
    let repo = TestRepo::new();
    repo.write(".smart-commit.toml", "[style]\nnope = 1\n");
    let output = home.doctor(&[], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(line(&stdout(&output), "fail  config").contains("style.nope"));
}

#[test]
fn thorough_is_accepted_and_still_checks_the_size_first() {
    // Hashing the pinned models takes minutes; the hash comparison is a unit
    // test over a small file.
    let home = Home::new();
    home.install();
    let model = models::find(models::DEFAULT).unwrap();
    let file = home
        .data
        .path()
        .join("smart-commit/models")
        .join(model.gguf.file);
    fs::File::create(&file).unwrap().set_len(10).unwrap();
    let output = home.doctor(&["--thorough"], home.data.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(line(&stdout(&output), "fail  model").contains("not the pinned"));
}
