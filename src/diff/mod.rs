//! Fits a staged diff into the small prompt an NPU model accepts.
//!
//! Files are classified by path, noise (lockfiles, generated and binary
//! files) is reduced to a stat line, and the remaining token budget is shared
//! between files by priority: source first, then config, tests and docs.

pub mod annotate;
mod classify;
mod compact;
mod parse;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use classify::machine_written;
pub use compact::{compact, compact_annotated};
pub use parse::parse;

/// Rough characters per token for code and pt/en prose with Qwen-style
/// tokenizers. Deliberately pessimistic: overflowing the NPU's fixed prompt
/// length fails the generation, while underfilling only loses some context.
pub const CHARS_PER_TOKEN: usize = 3;

/// Below this, a file's hunks are dropped instead of truncated: a few lines
/// of a large change mislead more than they help.
const MIN_FILE_TOKENS: usize = 24;
/// A cut file shows at least this many lines of its hunks besides the
/// headers, or it is left out: a header alone tells nothing the file list
/// does not.
const MIN_BODY_LINES: usize = 3;

pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    /// Original path, for renames and copies.
    pub old_path: Option<String>,
    /// Added and deleted lines; `None` for binary files.
    pub lines: Option<(usize, usize)>,
    /// The patch from the first `@@` on, without the `diff --git` header.
    pub hunks: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    // Declared in priority order.
    Source,
    Config,
    Test,
    Docs,
    Lockfile,
    Generated,
    Binary,
}

impl FileKind {
    pub fn of(file: &FileDiff) -> Self {
        if file.lines.is_none() {
            return Self::Binary;
        }
        Self::of_path(&file.path)
    }

    /// Classifies by path alone, which cannot tell binary files apart.
    pub fn of_path(path: &str) -> Self {
        classify::classify_path(path)
    }

    /// Whether the hunks are worth any of the budget.
    pub fn is_noise(self) -> bool {
        matches!(self, Self::Lockfile | Self::Generated | Self::Binary)
    }
}

impl FileDiff {
    /// Whether the file has hunks that may go into the prompt.
    fn has_useful_hunks(&self) -> bool {
        !self.hunks.is_empty() && !FileKind::of(self).is_noise()
    }

    /// The least a cut section costs: its headers, [`MIN_BODY_LINES`] lines
    /// and the marker, as [`fit_file`] counts them; at least
    /// [`MIN_FILE_TOKENS`], at most the whole section.
    fn min_tokens(&self) -> usize {
        let section = self.section();
        let lines: Vec<&str> = section.lines().collect();
        let mut cost = 0;
        let mut body = 0;
        for (n, line) in lines.iter().enumerate() {
            if body == MIN_BODY_LINES {
                cost += estimate_tokens(&format!("[... {} more lines]\n", lines.len() - n));
                break;
            }
            cost += estimate_tokens(line) + 1;
            body += usize::from(!compact::is_header(line));
        }
        cost.max(MIN_FILE_TOKENS).min(estimate_tokens(&section))
    }

    /// The file as it appears in the prompt's diff.
    fn section(&self) -> String {
        let mut text = format!("--- {}\n{}", self.path, self.hunks);
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    }
}

/// A diff reduced to fit a token budget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactDiff {
    /// One line per file, always complete for the files it lists.
    pub stat: String,
    /// The hunks that fit, grouped per file.
    pub patch: String,
    /// Files whose hunks were dropped or cut short.
    pub truncated: usize,
}
