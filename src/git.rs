//! Thin wrapper around the `git` binary.
//!
//! We shell out instead of using a git library so the user's own config
//! (diff drivers, `.gitattributes`, rename detection) is respected.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::diff::{self, FileDiff};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to run git: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("not a git repository: {0}")]
    NotARepository(PathBuf),
    #[error("`git {args}` failed: {stderr}")]
    Command { args: String, stderr: String },
    #[error("git output is not valid UTF-8")]
    Utf8(#[from] std::string::FromUtf8Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Other(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub status: ChangeStatus,
    pub path: String,
    /// Original path, for renames and copies.
    pub old_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
    /// Extra environment for every git call, e.g. to isolate tests from
    /// the user's config.
    env: Vec<(String, String)>,
}

impl Repo {
    /// Finds the repository containing `path`.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, GitError> {
        let path = path.as_ref();
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        if !output.status.success() {
            return Err(GitError::NotARepository(path.to_path_buf()));
        }
        let root = String::from_utf8(output.stdout)?;
        Ok(Self {
            root: PathBuf::from(root.trim_end()),
            env: Vec::new(),
        })
    }

    /// Sets environment variables for every git call made through `self`.
    pub fn with_env<K: Into<String>, V: Into<String>>(
        mut self,
        vars: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.env
            .extend(vars.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Subjects of the last `n` non-merge commits, most recent first.
    pub fn recent_subjects(&self, n: usize) -> Result<Vec<String>, GitError> {
        if !self.has_commits()? {
            return Ok(Vec::new());
        }
        let out = self.git(&["log", &format!("-n{n}"), "--no-merges", "--format=%s"])?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// Current branch name, or `None` on a detached HEAD.
    pub fn current_branch(&self) -> Result<Option<String>, GitError> {
        let out = self.git(&["branch", "--show-current"])?;
        let name = out.trim();
        Ok((!name.is_empty()).then(|| name.to_owned()))
    }

    /// Files in the index that differ from HEAD.
    pub fn staged_changes(&self) -> Result<Vec<FileChange>, GitError> {
        let out = self.git(&["diff", "--staged", "--name-status", "-z"])?;
        Ok(parse_name_status_z(&out))
    }

    /// The staged diff with one line of context, per file.
    pub fn staged_diff(&self) -> Result<Vec<FileDiff>, GitError> {
        // `--no-ext-diff` keeps external diff tools from replacing the
        // unified format we parse; textconv drivers still apply.
        let common = ["diff", "--staged", "--no-color", "--no-ext-diff"];
        let numstat = self.git(&[&common[..], &["--numstat", "-z"]].concat())?;
        let patch = self.git(&[&common[..], &["-U1"]].concat())?;
        Ok(diff::parse(&numstat, &patch))
    }

    fn has_commits(&self) -> Result<bool, GitError> {
        let output = self.run(&["rev-parse", "--verify", "--quiet", "HEAD"])?;
        Ok(output.status.success())
    }

    fn git(&self, args: &[&str]) -> Result<String, GitError> {
        let output = self.run(args)?;
        if !output.status.success() {
            return Err(GitError::Command {
                args: args.join(" "),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    fn run(&self, args: &[&str]) -> Result<Output, GitError> {
        Ok(Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()?)
    }
}

/// Parses `git diff --name-status -z` output.
///
/// Entries are NUL-separated: `STATUS\0PATH\0`, except renames and copies,
/// which carry a similarity score and two paths: `R100\0OLD\0NEW\0`.
fn parse_name_status_z(out: &str) -> Vec<FileChange> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut changes = Vec::new();
    while let Some(code) = fields.next() {
        let status = match code.chars().next() {
            Some('A') => ChangeStatus::Added,
            Some('M') => ChangeStatus::Modified,
            Some('D') => ChangeStatus::Deleted,
            Some('R') => ChangeStatus::Renamed,
            Some('C') => ChangeStatus::Copied,
            Some('T') => ChangeStatus::TypeChanged,
            Some(c) => ChangeStatus::Other(c),
            None => continue,
        };
        let old_path = match status {
            ChangeStatus::Renamed | ChangeStatus::Copied => fields.next().map(str::to_owned),
            _ => None,
        };
        let Some(path) = fields.next() else { break };
        changes.push(FileChange {
            status,
            path: path.to_owned(),
            old_path,
        });
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(status: ChangeStatus, path: &str, old_path: Option<&str>) -> FileChange {
        FileChange {
            status,
            path: path.to_owned(),
            old_path: old_path.map(str::to_owned),
        }
    }

    #[test]
    fn parses_simple_statuses() {
        let out = "A\0new.rs\0M\0src/lib.rs\0D\0old.txt\0T\0link\0";
        assert_eq!(
            parse_name_status_z(out),
            vec![
                change(ChangeStatus::Added, "new.rs", None),
                change(ChangeStatus::Modified, "src/lib.rs", None),
                change(ChangeStatus::Deleted, "old.txt", None),
                change(ChangeStatus::TypeChanged, "link", None),
            ]
        );
    }

    #[test]
    fn parses_renames_and_copies_with_both_paths() {
        let out = "R087\0a.rs\0b.rs\0C100\0x.rs\0y.rs\0M\0z.rs\0";
        assert_eq!(
            parse_name_status_z(out),
            vec![
                change(ChangeStatus::Renamed, "b.rs", Some("a.rs")),
                change(ChangeStatus::Copied, "y.rs", Some("x.rs")),
                change(ChangeStatus::Modified, "z.rs", None),
            ]
        );
    }

    #[test]
    fn keeps_paths_with_spaces_and_newlines() {
        let out = "A\0my file\nwith newline.txt\0";
        assert_eq!(
            parse_name_status_z(out),
            vec![change(
                ChangeStatus::Added,
                "my file\nwith newline.txt",
                None
            )]
        );
    }

    #[test]
    fn unknown_status_is_preserved() {
        assert_eq!(
            parse_name_status_z("U\0conflict.rs\0"),
            vec![change(ChangeStatus::Other('U'), "conflict.rs", None)]
        );
    }

    #[test]
    fn empty_output_means_no_changes() {
        assert!(parse_name_status_z("").is_empty());
    }

    #[test]
    fn truncated_output_is_ignored() {
        assert!(parse_name_status_z("M\0").is_empty());
    }
}
