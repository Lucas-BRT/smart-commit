//! The annotated file list: what each staged file's line counts do not say.
//!
//! The plain list (`path | +3 -1`) leaves the model to guess a file's
//! status, and shows a whitespace fix, a comment and a rename like any
//! change. The annotated one names them: `new`, `deleted`, `renamed` (as
//! `dir/{old => new}/file`, one line for many files moved alike),
//! `whitespace only`, `comments only`, `no line changed`, and a new file
//! whose content is mostly a deleted one's, which git did not pair.

mod comments;
mod rename;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::diff::FileDiff;
use crate::git::{ChangeStatus, FileChange};
use comments::CommentSyntax;
use rename::{likely_source, moved_between};

pub use rename::rename_path;

/// Renames between the same two directories, from this many files, take
/// one line of the file list.
const GROUP_RENAMES: usize = 3;

/// What the lines of a file's hunks change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Content,
    /// The same text once whitespace is ignored.
    Whitespace(Spacing),
    /// Only comments, docstrings or blank lines.
    Comments,
    /// No line at all: a mode or type change, an empty file, a pure rename.
    NoLines,
}

/// Which whitespace a whitespace-only change touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    /// `\r\n` became `\n` (`to_lf`) or the reverse.
    LineEndings {
        to_lf: bool,
    },
    FinalNewline,
    TrailingSpaces,
    Indentation,
    BlankLines,
    /// Lines split or joined: code reformatted.
    LineBreaks,
}

impl Spacing {
    fn describe(self) -> &'static str {
        match self {
            Self::LineEndings { to_lf: true } => "CRLF to LF",
            Self::LineEndings { to_lf: false } => "LF to CRLF",
            Self::FinalNewline => "final newline",
            Self::TrailingSpaces => "trailing spaces",
            Self::Indentation => "indentation",
            Self::BlankLines => "blank lines",
            Self::LineBreaks => "line breaks",
        }
    }

    /// Whether the lines tell more than the description: only for code
    /// reformatted, where they show which code it is. The other changes
    /// are invisible in a diff.
    pub fn shows_in_diff(self) -> bool {
        self == Self::LineBreaks
    }
}

/// Which whitespace changed between `removed` and `added`, lines whose
/// text is the same once whitespace is ignored.
fn spacing_of(hunks: &str, removed: &[&str], added: &[&str]) -> Spacing {
    let pairwise = |same: fn(&str, &str) -> bool| {
        removed.len() == added.len() && removed.iter().zip(added).all(|(r, a)| same(r, a))
    };
    let cr = |lines: &[&str]| lines.iter().filter(|l| l.ends_with('\r')).count();
    let (cr_removed, cr_added) = (cr(removed), cr(added));
    if cr_removed != cr_added
        && pairwise(|r, a| r.strip_suffix('\r').unwrap_or(r) == a.strip_suffix('\r').unwrap_or(a))
    {
        return Spacing::LineEndings {
            to_lf: cr_removed > cr_added,
        };
    }
    if hunks.contains("\n\\ No newline at end of file") && pairwise(|r, a| r == a) {
        return Spacing::FinalNewline;
    }
    if pairwise(|r, a| r.trim_end() == a.trim_end()) {
        return Spacing::TrailingSpaces;
    }
    if pairwise(|r, a| r.trim_start() == a.trim_start()) {
        return Spacing::Indentation;
    }
    if removed.iter().chain(added).all(|l| l.trim().is_empty()) {
        return Spacing::BlankLines;
    }
    Spacing::LineBreaks
}

/// Classifies `file` from its hunks; binary files are content.
pub fn change_of(file: &FileDiff) -> Change {
    if file.lines.is_none() {
        return Change::Content;
    }
    let (removed, added) = changed_lines(&file.hunks);
    if removed.is_empty() && added.is_empty() {
        return Change::NoLines;
    }
    let squeeze = |lines: &[&str]| -> String {
        lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    if squeeze(&removed) == squeeze(&added) {
        return Change::Whitespace(spacing_of(&file.hunks, &removed, &added));
    }
    let Some(syntax) = CommentSyntax::of(&file.path) else {
        return Change::Content;
    };
    if syntax.only_comments(&removed) && syntax.only_comments(&added) {
        Change::Comments
    } else {
        Change::Content
    }
}

/// The removed and added lines of `hunks`, without their `-`/`+`. Split
/// on `\n` alone: `str::lines` would drop the `\r` of CRLF lines, and with
/// it the difference of a line-ending change.
fn changed_lines(hunks: &str) -> (Vec<&str>, Vec<&str>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for line in hunks.split('\n') {
        if let Some(text) = line.strip_prefix('-') {
            removed.push(text);
        } else if let Some(text) = line.strip_prefix('+') {
            added.push(text);
        }
    }
    (removed, added)
}

fn counts(lines: Option<(usize, usize)>) -> String {
    match lines {
        Some((added, deleted)) => format!("+{added} -{deleted}"),
        None => "binary".to_owned(),
    }
}

/// One line per file, or per group of files moved alike, in the order of
/// `files` (a group where its first file is).
pub fn stat_lines(files: &[FileDiff], staged: &[FileChange]) -> Vec<String> {
    let status = |path: &str| {
        staged
            .iter()
            .find(|c| c.path == path)
            .map(|c| (c.status, c.old_path.as_deref()))
    };
    // Renames between the same two directories, by directory pair; a
    // BTreeMap keeps the output independent of hashing.
    let mut moves: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, file) in files.iter().enumerate() {
        if let Some(pair) = file
            .old_path
            .as_deref()
            .and_then(|old| moved_between(old, &file.path))
        {
            moves.entry(pair).or_default().push(i);
        }
    }
    let mut grouped = vec![None; files.len()];
    for ((old_dir, new_dir), members) in &moves {
        if members.len() >= GROUP_RENAMES {
            for &i in members {
                grouped[i] = Some((old_dir.clone(), new_dir.clone(), members[0]));
            }
        }
    }
    let deleted: Vec<&FileDiff> = files
        .iter()
        .filter(|f| matches!(status(&f.path), Some((ChangeStatus::Deleted, _))))
        .collect();

    let mut lines = Vec::new();
    for (i, file) in files.iter().enumerate() {
        if let Some((old_dir, new_dir, first)) = &grouped[i] {
            if *first != i {
                continue;
            }
            let members = &moves[&(old_dir.clone(), new_dir.clone())];
            let (added, removed) = members
                .iter()
                .filter_map(|&m| files[m].lines)
                .fold((0, 0), |(a, d), (fa, fd)| (a + fa, d + fd));
            let dirs = rename_path(&format!("{old_dir}/x"), &format!("{new_dir}/x"));
            let dirs = dirs.strip_suffix("/x").unwrap_or(&dirs);
            lines.push(format!(
                "{dirs}/ | {} files renamed, +{added} -{removed}\n",
                members.len()
            ));
            continue;
        }
        let change = change_of(file);
        let mut words: Vec<String> = Vec::new();
        let path = match (&file.old_path, status(&file.path)) {
            (Some(old), Some((ChangeStatus::Copied, _))) => {
                words.push(format!("copied from {old}"));
                file.path.clone()
            }
            (Some(old), _) => {
                words.push("renamed".to_owned());
                rename_path(old, &file.path)
            }
            (None, Some((ChangeStatus::Added, _))) => {
                words.push("new".to_owned());
                if let Some(source) = likely_source(file, &deleted) {
                    words.push(format!("maybe renamed from {source}"));
                }
                file.path.clone()
            }
            (None, Some((ChangeStatus::Deleted, _))) => {
                words.push("deleted".to_owned());
                file.path.clone()
            }
            (None, Some((ChangeStatus::TypeChanged, _))) => {
                words.push("type changed".to_owned());
                file.path.clone()
            }
            _ => file.path.clone(),
        };
        match change {
            Change::Whitespace(spacing) => {
                words.push(format!("whitespace only: {}", spacing.describe()));
            }
            Change::Comments => words.push("comments only".to_owned()),
            Change::NoLines if file.old_path.is_some() => words.push("unchanged".to_owned()),
            Change::NoLines if words.first().is_some_and(|w| w == "new") => {
                words.push("empty".to_owned());
            }
            Change::NoLines if words.is_empty() => words.push("no line changed (mode)".to_owned()),
            _ => {}
        }
        words.push(counts(file.lines));
        lines.push(format!("{path} | {}\n", words.join(", ")));
    }
    lines
}
