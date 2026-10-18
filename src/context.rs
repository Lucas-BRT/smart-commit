//! Everything we know about the repository before writing a prompt.

use std::path::PathBuf;

use crate::git::{FileChange, GitError, Repo};
use crate::style::{self, Style};

/// How many recent commits the style is learned from, by default.
pub const HISTORY_DEPTH: usize = 30;

#[derive(Debug, Clone)]
pub struct RepoContext {
    pub root: PathBuf,
    pub branch: Option<String>,
    /// Issue key found in the branch name, e.g. `ABC-123`.
    pub ticket: Option<String>,
    /// Subjects the style was learned from, most recent first.
    pub subjects: Vec<String>,
    pub style: Style,
    pub staged: Vec<FileChange>,
}

impl RepoContext {
    pub fn collect(repo: &Repo) -> Result<Self, GitError> {
        Self::collect_with_history(repo, HISTORY_DEPTH)
    }

    /// Like [`Self::collect`], learning the style from the last
    /// `history_depth` commits.
    pub fn collect_with_history(repo: &Repo, history_depth: usize) -> Result<Self, GitError> {
        Ok(Self::new(
            repo.root().to_path_buf(),
            repo.current_branch()?,
            repo.recent_subjects(history_depth)?,
            repo.staged_changes()?,
        ))
    }

    /// Derives everything else from what git reports, so a context can be
    /// rebuilt exactly from recorded data.
    pub fn new(
        root: PathBuf,
        branch: Option<String>,
        subjects: Vec<String>,
        staged: Vec<FileChange>,
    ) -> Self {
        Self {
            root,
            ticket: branch.as_deref().and_then(ticket_from_branch),
            branch,
            style: style::detect(&subjects),
            subjects,
            staged,
        }
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
    fn new_derives_ticket_and_style_from_raw_data() {
        let subjects = vec![
            "feat(api): add login".to_owned(),
            "fix(api): crash".to_owned(),
        ];
        let ctx = RepoContext::new(
            PathBuf::from("/repo"),
            Some("feature/ABC-9-login".into()),
            subjects.clone(),
            Vec::new(),
        );
        assert_eq!(ctx.ticket.as_deref(), Some("ABC-9"));
        assert_eq!(ctx.style, style::detect(&subjects));
        assert_eq!(ctx.subjects, subjects);
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
