//! Everything we know about the repository before writing a prompt.

use std::path::PathBuf;

use crate::git::{FileChange, GitError, Repo};
use crate::style::{self, Style};

/// How many recent commits the style is learned from.
pub const HISTORY_DEPTH: usize = 30;

#[derive(Debug, Clone)]
pub struct RepoContext {
    pub root: PathBuf,
    pub branch: Option<String>,
    /// Issue key found in the branch name, e.g. `ABC-123`.
    pub ticket: Option<String>,
    pub style: Style,
    pub staged: Vec<FileChange>,
}

impl RepoContext {
    pub fn collect(repo: &Repo) -> Result<Self, GitError> {
        let branch = repo.current_branch()?;
        Ok(Self {
            root: repo.root().to_path_buf(),
            ticket: branch.as_deref().and_then(ticket_from_branch),
            branch,
            style: style::detect(&repo.recent_subjects(HISTORY_DEPTH)?),
            staged: repo.staged_changes()?,
        })
    }
}

/// Finds a Jira-style issue key (`ABC-123`) in a branch name.
pub fn ticket_from_branch(branch: &str) -> Option<String> {
    style::find_ticket(branch).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ticket_in_common_branch_names() {
        for (branch, ticket) in [
            ("ABC-123", "ABC-123"),
            ("feature/ABC-123-login-page", "ABC-123"),
            ("fix/PROJ2-7_crash", "PROJ2-7"),
            ("lucas/OPS-42", "OPS-42"),
        ] {
            assert_eq!(
                ticket_from_branch(branch).as_deref(),
                Some(ticket),
                "{branch}"
            );
        }
    }

    #[test]
    fn ignores_branches_without_ticket() {
        for branch in [
            "main",
            "feature/login",
            "fix-2",
            "abc-123",
            "ABC-",
            "ABC-12x",
            "xABC-12",
            "-123",
        ] {
            assert_eq!(ticket_from_branch(branch), None, "{branch}");
        }
    }
}
