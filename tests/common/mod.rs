//! A throwaway git repository for integration tests.

// Each test binary uses a different subset of the helpers.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::Command;

use npu_commit::context::RepoContext;
use npu_commit::git::Repo;
use tempfile::TempDir;

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
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    pub fn commit(&self, subject: &str) {
        self.git(&["commit", "--quiet", "--allow-empty", "-m", subject]);
    }

    pub fn write(&self, file: &str, contents: &str) {
        fs::write(self.path().join(file), contents).unwrap();
    }

    pub fn repo(&self) -> Repo {
        Repo::discover(self.path()).unwrap()
    }

    pub fn context(&self) -> RepoContext {
        RepoContext::collect(&self.repo()).unwrap()
    }
}
