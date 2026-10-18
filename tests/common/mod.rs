//! A throwaway git repository for integration tests.

// Each test binary uses a different subset of the helpers.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use smart_commit::context::RepoContext;
use smart_commit::git::Repo;
use tempfile::TempDir;

/// Keeps git away from the developer's config and pins identities and dates,
/// so commits (hashes included) are the same on every machine.
pub const GIT_ENV: &[(&str, &str)] = &[
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_AUTHOR_NAME", "Test"),
    ("GIT_AUTHOR_EMAIL", "test@example.com"),
    ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
    ("GIT_COMMITTER_NAME", "Test"),
    ("GIT_COMMITTER_EMAIL", "test@example.com"),
    ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
];

/// Where the binary looks for config, models and caches during tests:
/// nowhere real, so the developer's own config and exported model are never
/// picked up. Tests that write config pass their own XDG_CONFIG_HOME.
pub const CONFIG_HOME: &str = "/nonexistent/smart-commit-tests/config";
pub const DATA_HOME: &str = "/nonexistent/smart-commit-tests/data";
pub const CACHE_HOME: &str = "/nonexistent/smart-commit-tests/cache";

/// Applies [`GIT_ENV`], drops every `SMART_COMMIT_*` variable the developer
/// may have exported (settings and test overrides alike) and points the XDG directories at
/// [`CONFIG_HOME`], [`DATA_HOME`] and [`CACHE_HOME`], so the command behaves
/// the same on every machine.
pub fn isolate(command: &mut Command) -> &mut Command {
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("SMART_COMMIT_") {
            command.env_remove(var);
        }
    }
    command
        .env("XDG_CONFIG_HOME", CONFIG_HOME)
        .env("XDG_DATA_HOME", DATA_HOME)
        .env("XDG_CACHE_HOME", CACHE_HOME)
        .envs(GIT_ENV.iter().copied())
}

/// Runs the `smart-commit` binary, isolated like [`TestRepo::git`].
pub fn smart_commit(args: &[&str], dir: &Path, env: &[(&str, &str)]) -> Output {
    isolate(
        Command::new(env!("CARGO_BIN_EXE_smart-commit"))
            .args(args)
            .current_dir(dir),
    )
    .envs(env.iter().copied())
    .output()
    .unwrap()
}

pub struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    pub fn new() -> Self {
        let repo = Self {
            dir: TempDir::new().unwrap(),
        };
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs git isolated from the developer's own config.
    pub fn git(&self, args: &[&str]) {
        self.git_output(args);
    }

    /// Runs git like [`Self::git`] and returns its stdout.
    pub fn git_output(&self, args: &[&str]) -> String {
        let output = self.git_with(args, &[]);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// Runs git with extra environment, without checking the outcome.
    pub fn git_with(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        isolate(Command::new("git").arg("-C").arg(self.path()).args(args))
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }

    pub fn commit(&self, subject: &str) {
        self.git(&["commit", "--quiet", "--allow-empty", "-m", subject]);
    }

    pub fn write(&self, file: &str, contents: &str) {
        fs::write(self.path().join(file), contents).unwrap();
    }

    /// The repository as the crate sees it, isolated like [`Self::git`].
    pub fn repo(&self) -> Repo {
        Repo::discover(self.path())
            .unwrap()
            .with_env(GIT_ENV.iter().copied())
    }

    pub fn context(&self) -> RepoContext {
        RepoContext::collect(&self.repo()).unwrap()
    }
}
