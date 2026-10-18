//! A throwaway git repository for integration tests.

// Each test binary uses a different subset of the helpers.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::Command;

use npu_commit::context::RepoContext;
use npu_commit::git::Repo;
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
        let status = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .envs(GIT_ENV.iter().copied())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    /// Runs git like [`Self::git`] and returns its stdout.
    pub fn git_output(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .envs(GIT_ENV.iter().copied())
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8(output.stdout).unwrap()
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
