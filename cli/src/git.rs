//! Thin wrapper around the `git` binary.
//!
//! We shell out instead of using a git library so the user's own config
//! (diff drivers, `.gitattributes`, rename detection) is respected.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

use serde::{Deserialize, Serialize};

use crate::diff::{self, FileDiff};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to run git")]
    Spawn(#[from] std::io::Error),
    #[error("not a git repository: {0}")]
    NotARepository(PathBuf),
    #[error("`git {args}` failed: {stderr}")]
    Command { args: String, stderr: String },
    #[error("`git {args}` exited with {status}")]
    Exited { args: String, status: ExitStatus },
    #[error("git output is not valid UTF-8")]
    Utf8(#[from] std::string::FromUtf8Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Other(char),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub status: ChangeStatus,
    pub path: String,
    /// Original path, for renames and copies.
    pub old_path: Option<String>,
}

/// What git is in the middle of when it prepared a commit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Merge,
    Revert,
    CherryPick,
}

/// The message git prepared in `MERGE_MSG` for a merge, revert or
/// cherry-pick in progress, the one `git commit` would use; comments out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedMessage {
    pub operation: Operation,
    pub message: String,
}

/// Paths kept per history commit: enough to tell the areas a scope covers.
pub const HISTORY_FILES: usize = 20;

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
        Ok(self.recent_commits(n)?.into_iter().map(|c| c.0).collect())
    }

    /// The last `n` non-merge commits, most recent first: each subject with
    /// the paths it touched (new paths of renames, old ones of deletions),
    /// sorted, at most [`HISTORY_FILES`] of them.
    pub fn recent_commits(&self, n: usize) -> Result<Vec<(String, Vec<String>)>, GitError> {
        if !self.has_commits()? {
            return Ok(Vec::new());
        }
        // Records start with 0x1e; then the subject, NUL, and NUL-separated
        // paths (after a newline).
        let out = self.git(&[
            "log",
            &format!("-n{n}"),
            "--no-merges",
            "--format=%x1e%s",
            "--name-only",
            "-z",
        ])?;
        Ok(out
            .split('\x1e')
            .skip(1)
            .map(|record| {
                let (subject, rest) = record.split_once('\0').unwrap_or((record, ""));
                let mut paths: Vec<String> = rest
                    .split('\0')
                    .map(|p| p.trim_start_matches('\n'))
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect();
                paths.sort();
                paths.dedup();
                paths.truncate(HISTORY_FILES);
                (subject.to_owned(), paths)
            })
            .collect())
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

    /// The paths among `paths` that `.gitattributes` marks
    /// `linguist-generated`, read from the index so that what is staged
    /// decides.
    pub fn generated_paths(&self, paths: &[&str]) -> Result<Vec<String>, GitError> {
        let mut generated = Vec::new();
        // In chunks, to stay far from the command line's length limit.
        for chunk in paths.chunks(256) {
            let mut args = vec!["check-attr", "--cached", "-z", "linguist-generated", "--"];
            args.extend_from_slice(chunk);
            let out = self.git(&args)?;
            // NUL-separated triplets: path, attribute, value.
            let fields: Vec<&str> = out.split('\0').collect();
            generated.extend(
                fields
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter(|[_, _, value]| matches!(*value, "set" | "true"))
                    .map(|[path, _, _]| (*path).to_owned()),
            );
        }
        Ok(generated)
    }

    /// Where git looks for hooks, honouring `core.hooksPath`.
    pub fn hooks_dir(&self) -> Result<PathBuf, GitError> {
        let out = self.git(&["rev-parse", "--git-path", "hooks"])?;
        // Relative paths are relative to where git ran: the root.
        Ok(self.root.join(out.trim_end()))
    }

    /// The message git prepared for the commit in progress, if any: a merge
    /// (`MERGE_HEAD`), a revert (`REVERT_HEAD`) or a cherry-pick
    /// (`CHERRY_PICK_HEAD`, or none with `--no-commit`) leave it in
    /// `MERGE_MSG`. `git merge --squash` writes `SQUASH_MSG` instead, a list
    /// of the squashed commits rather than a subject: not used.
    pub fn prepared_message(&self) -> Result<Option<PreparedMessage>, GitError> {
        let names = ["MERGE_MSG", "REVERT_HEAD", "CHERRY_PICK_HEAD", "MERGE_HEAD"];
        let mut args = vec!["rev-parse"];
        for name in names {
            args.extend(["--git-path", name]);
        }
        let out = self.git(&args)?;
        // Relative paths are relative to where git ran: the root.
        let paths: Vec<PathBuf> = out.lines().map(|line| self.root.join(line)).collect();
        let text = match std::fs::read_to_string(&paths[0]) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let message = without_comments(&text, &self.comment_prefix()?);
        if message.is_empty() {
            return Ok(None);
        }
        let operation = if paths[1].exists() {
            Operation::Revert
        } else if paths[3].exists() {
            Operation::Merge
        } else {
            Operation::CherryPick
        };
        Ok(Some(PreparedMessage { operation, message }))
    }

    /// The prefix of comment lines in commit messages (`core.commentChar`).
    ///
    /// `auto` makes git pick a character per message; we assume the default
    /// then, which only matters for messages that start with `#`.
    pub fn comment_prefix(&self) -> Result<String, GitError> {
        let output = self.run(&["config", "--get", "core.commentChar"])?;
        let value = String::from_utf8(output.stdout)?;
        let value = value.trim_end();
        Ok(match value {
            "" | "auto" => "#".to_owned(),
            custom => custom.to_owned(),
        })
    }

    /// Runs `git commit` with `message` pre-filled in the user's editor.
    ///
    /// Git's own terminal handling applies: hooks run, an empty message
    /// aborts, and the editor gets the terminal.
    /// Commits what is staged with `message`, as `git commit --message`:
    /// hooks run, and git's output reaches the terminal.
    pub fn commit(&self, message: &str) -> Result<(), GitError> {
        let status = self.command(&["commit", "--message", message]).status()?;
        if !status.success() {
            return Err(GitError::Exited {
                args: "commit --message".to_owned(),
                status,
            });
        }
        Ok(())
    }

    pub fn commit_in_editor(&self, message: &str) -> Result<(), GitError> {
        let status = self
            .command(&["commit", "--edit", "--message", message])
            .status()?;
        if !status.success() {
            return Err(GitError::Exited {
                args: "commit --edit".to_owned(),
                status,
            });
        }
        Ok(())
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
        Ok(self.command(args).output()?)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }
}

/// `text` without its comment lines and trailing whitespace, as `git
/// commit` cleans a message by default.
fn without_comments(text: &str, comment: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with(comment))
        .map(str::trim_end)
        .collect();
    let mut message = String::new();
    for (n, line) in lines.iter().enumerate() {
        // Runs of blank lines become one, as `git commit` does.
        if line.is_empty() && (n == 0 || lines[n - 1].is_empty()) {
            continue;
        }
        message.push_str(line);
        message.push('\n');
    }
    message.trim().to_owned()
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
    use std::error::Error as _;

    use super::*;

    #[test]
    fn a_wrapped_error_is_the_source_not_part_of_the_message() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = GitError::from(io);
        assert_eq!(err.to_string(), "failed to run git");
        assert_eq!(err.source().unwrap().to_string(), "no such file");
    }

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
    fn prepared_messages_lose_their_comments() {
        let merge = "Merge branch 'side'\n\n# Conflicts:\n#\tsrc/a.rs\n";
        assert_eq!(without_comments(merge, "#"), "Merge branch 'side'");
        let revert = "Revert \"feat: x\"\n\nThis reverts commit 2e2ce44.  \n\n\n";
        assert_eq!(
            without_comments(revert, "#"),
            "Revert \"feat: x\"\n\nThis reverts commit 2e2ce44."
        );
        assert_eq!(without_comments("; note\nfix: y\n", ";"), "fix: y");
        assert_eq!(without_comments("# only a comment\n", "#"), "");
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
