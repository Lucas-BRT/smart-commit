//! Everything we know about the repository before writing a prompt.

use std::path::PathBuf;

use crate::git::{FileChange, GitError, PreparedMessage, Repo};
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
    /// The paths each of `subjects` touched; empty when not collected
    /// (traces written before it was).
    pub history_files: Vec<Vec<String>>,
    pub style: Style,
    pub staged: Vec<FileChange>,
    /// Staged paths `.gitattributes` marks `linguist-generated`.
    pub generated: Vec<String>,
    /// The message git prepared for a merge, revert or cherry-pick in
    /// progress.
    pub prepared: Option<PreparedMessage>,
}

impl RepoContext {
    pub fn collect(repo: &Repo) -> Result<Self, GitError> {
        Self::collect_with_history(repo, HISTORY_DEPTH)
    }

    /// Like [`Self::collect`], learning the style from the last
    /// `history_depth` commits.
    pub fn collect_with_history(repo: &Repo, history_depth: usize) -> Result<Self, GitError> {
        let (subjects, files) = repo.recent_commits(history_depth)?.into_iter().unzip();
        let staged = repo.staged_changes()?;
        let paths: Vec<&str> = staged.iter().map(|c| c.path.as_str()).collect();
        let generated = repo.generated_paths(&paths)?;
        Ok(Self::new(
            repo.root().to_path_buf(),
            repo.current_branch()?,
            subjects,
            staged,
        )
        .with_history_files(files)
        .with_generated(generated)
        .with_prepared(repo.prepared_message()?))
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
            history_files: Vec::new(),
            staged,
            generated: Vec::new(),
            prepared: None,
        }
    }

    /// Adds the paths each history subject touched, which tell the areas
    /// each scope covers.
    pub fn with_history_files(mut self, files: Vec<Vec<String>>) -> Self {
        self.history_files = files;
        self
    }

    /// Adds the staged paths marked `linguist-generated`.
    pub fn with_generated(mut self, generated: Vec<String>) -> Self {
        self.generated = generated;
        self
    }

    /// Adds the message git prepared for the commit in progress.
    pub fn with_prepared(mut self, prepared: Option<PreparedMessage>) -> Self {
        self.prepared = prepared;
        self
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
