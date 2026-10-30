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

/// Letters per token in long runs mixing letters and digits (hashes,
/// base64, keys), which split into short tokens.
const DENSE_LETTERS_PER_TOKEN: usize = 2;

/// Runs at least this long, of letters and digits mixed, count as dense.
const DENSE_RUN: usize = 16;

/// Below this, a file's hunks are dropped instead of truncated: a few lines
/// of a large change mislead more than they help.
const MIN_FILE_TOKENS: usize = 24;
/// A cut file shows at least this many lines of its hunks besides the
/// headers, or it is left out: a header alone tells nothing the file list
/// does not.
const MIN_BODY_LINES: usize = 3;

/// The tokens of `text` for Qwen's tokenizer, from above for most text:
/// [`CHARS_PER_TOKEN`] characters per token, except that every digit is a
/// token of its own and the letters of dense runs (hashes, base64) go
/// [`DENSE_LETTERS_PER_TOKEN`] per token. Characters / 3 alone was well
/// short on such diffs; the backends' headroom covers what is left (see
/// `docs/measurements.md`).
pub fn estimate_tokens(text: &str) -> usize {
    // (digits, letters of dense runs, other characters)
    let mut counts = (0, 0, 0);
    let mut run_start = None;
    let add_run = |run: &str, counts: &mut (usize, usize, usize)| {
        let digits = run.chars().filter(char::is_ascii_digit).count();
        let letters = run.chars().filter(char::is_ascii_alphabetic).count();
        let len = run.chars().count();
        counts.0 += digits;
        if len >= DENSE_RUN && digits > 0 && letters > 0 {
            counts.1 += letters;
            counts.2 += len - digits - letters;
        } else {
            counts.2 += len - digits;
        }
    };
    for (i, c) in text.char_indices() {
        let in_run = c.is_ascii_alphanumeric() || "+/=_-".contains(c);
        match (in_run, run_start) {
            (true, None) => run_start = Some(i),
            (false, Some(start)) => {
                add_run(&text[start..i], &mut counts);
                run_start = None;
                counts.2 += 1;
            }
            (false, None) => counts.2 += 1,
            (true, Some(_)) => {}
        }
    }
    if let Some(start) = run_start {
        add_run(&text[start..], &mut counts);
    }
    let (digits, dense_letters, other) = counts;
    digits + dense_letters.div_ceil(DENSE_LETTERS_PER_TOKEN) + other.div_ceil(CHARS_PER_TOKEN)
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
