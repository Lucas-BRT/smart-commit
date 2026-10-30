//! Cleans up what the model writes, and the message used when it fails.
//!
//! Small models wrap the answer in code fences, quote it, announce it
//! ("Here is the commit message:"), explain it afterwards, emit Qwen3
//! `<think>` blocks or overflow 72 columns. All of that is fixed here,
//! deterministically, so the backend only has to get the content right.

mod fallback;
mod format;
mod parse;
#[cfg(test)]
mod tests;

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::style::Convention;

pub use fallback::{fallback, fallback_after};
pub use format::{fix_conventional, truncate_subject, wrap_body};

pub const SUBJECT_MAX: usize = 72;
pub const BODY_WIDTH: usize = 72;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMessage {
    pub subject: String,
    pub body: Option<String>,
}

impl fmt::Display for CommitMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.subject)?;
        if let Some(body) = &self.body {
            write!(f, "\n\n{body}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SanitizeError {
    #[error("the model returned no message")]
    Empty,
    #[error("subject does not follow Conventional Commits: {0:?}")]
    NotConventional(String),
    /// The subject repeats one from the history, as small models do when
    /// the history is short or the diff says little.
    #[error("subject repeats an earlier commit: {0:?}")]
    CopiedHistory(String),
    /// The subject repeats, or nearly, a message the user asked to replace:
    /// with greedy decoding the note asking for another is not always
    /// enough.
    #[error("subject repeats a message the user turned down: {0:?}")]
    TurnedDown(String),
    /// The subject stops in the middle of a sentence, after an article, a
    /// preposition or a conjunction: a reply cut off by the token limit.
    #[error("subject ends in the middle of a sentence: {0:?}")]
    CutOff(String),
}

pub fn sanitize(raw: &str, convention: Convention) -> Result<CommitMessage, SanitizeError> {
    sanitize_within(raw, convention, SUBJECT_MAX)
}

/// Like [`sanitize`], fitting the subject in `subject_max` columns.
pub fn sanitize_within(
    raw: &str,
    convention: Convention,
    subject_max: usize,
) -> Result<CommitMessage, SanitizeError> {
    let text = parse::strip_thinking(raw);
    let text = parse::unfence(&text);

    let mut lines = text
        .lines()
        .map(str::trim_end)
        .skip_while(|l| l.trim().is_empty() || parse::is_preamble(l));
    let subject = lines.next().map(parse::clean_subject).unwrap_or_default();
    if !subject.chars().any(char::is_alphanumeric) {
        return Err(SanitizeError::Empty);
    }
    let subject = match convention {
        Convention::Conventional => format::fix_conventional(&subject)?,
        _ => subject,
    };

    let body_lines: Vec<&str> = lines.take_while(|l| !parse::is_explanation(l)).collect();
    let body = format::wrap_body(&body_lines);
    Ok(CommitMessage {
        subject: format::truncate_subject(&subject, subject_max),
        body: (!body.is_empty()).then_some(body),
    })
}
