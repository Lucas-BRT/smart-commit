//! The annotated file list: what each staged file's line counts do not say.
//!
//! The plain list (`path | +3 -1`) leaves the model to guess a file's
//! status, and shows a whitespace fix, a comment and a rename like any
//! change. The annotated one names them: `new`, `deleted`, `renamed` (as
//! `dir/{old => new}/file`, one line for many files moved alike),
//! `whitespace only`, `comments only`, `no line changed`, and a new file
//! whose content is mostly a deleted one's, which git did not pair.

use std::collections::BTreeMap;

use crate::diff::FileDiff;
use crate::git::{ChangeStatus, FileChange};

/// Renames between the same two directories, from this many files, take
/// one line of the file list.
const GROUP_RENAMES: usize = 3;
/// A new file sharing this share of its lines with a deleted one is
/// probably that file renamed, below git's 50% rename threshold.
const LIKELY_RENAME: f64 = 0.4;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentSyntax {
    /// `#`, with Python's docstrings.
    Python,
    /// `#` alone: shell, Ruby, TOML, YAML.
    Hash,
    /// `//` and `/* ... */`.
    Slash,
}

impl CommentSyntax {
    fn of(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.').map_or("", |(_, e)| e);
        match extension {
            "py" | "pyi" => Some(Self::Python),
            "sh" | "bash" | "rb" | "toml" | "yaml" | "yml" | "pl" | "r" => Some(Self::Hash),
            "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "go" | "c" | "h" | "cc" | "cpp"
            | "hpp" | "java" | "kt" | "swift" | "cs" | "scala" | "dart" | "php" => {
                Some(Self::Slash)
            }
            _ => None,
        }
    }

    /// Whether every line is blank or a comment (true for none: a side
    /// with no line, as when comments are only added). Lines are read in
    /// order, so a docstring or a block comment spanning several changed
    /// lines counts whole.
    fn only_comments(self, lines: &[&str]) -> bool {
        let mut open = false;
        for line in lines {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            let comment = match self {
                Self::Hash => text.starts_with('#'),
                Self::Python => {
                    let quotes = text.matches("\"\"\"").count() + text.matches("'''").count();
                    let docstring = open || text.starts_with("\"\"\"") || text.starts_with("'''");
                    if quotes % 2 == 1 {
                        open = !open;
                    }
                    text.starts_with('#') || docstring
                }
                Self::Slash => {
                    let comment = open
                        || text.starts_with("//")
                        || text.starts_with("/*")
                        || text.starts_with('*');
                    if text.contains("/*") && !text.contains("*/") {
                        open = true;
                    } else if text.contains("*/") {
                        open = false;
                    }
                    comment
                }
            };
            if !comment {
                return false;
            }
        }
        true
    }
}

/// `dir/{old => new}/rest`, git's short form of a rename: the common
/// leading and trailing directories are written once.
pub fn rename_path(old: &str, new: &str) -> String {
    let old_parts: Vec<&str> = old.split('/').collect();
    let new_parts: Vec<&str> = new.split('/').collect();
    let mut head = 0;
    while head + 1 < old_parts.len().min(new_parts.len()) && old_parts[head] == new_parts[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < old_parts.len() - head - 1
        && tail < new_parts.len() - head - 1
        && old_parts[old_parts.len() - 1 - tail] == new_parts[new_parts.len() - 1 - tail]
    {
        tail += 1;
    }
    let join = |parts: &[&str]| parts.join("/");
    let prefix = join(&old_parts[..head]);
    let suffix = join(&old_parts[old_parts.len() - tail..]);
    let middle = format!(
        "{{{} => {}}}",
        join(&old_parts[head..old_parts.len() - tail]),
        join(&new_parts[head..new_parts.len() - tail])
    );
    let mut path = String::new();
    if !prefix.is_empty() {
        path.push_str(&prefix);
        path.push('/');
    }
    path.push_str(&middle);
    if !suffix.is_empty() {
        path.push('/');
        path.push_str(&suffix);
    }
    path
}

/// The directories a rename moves a file between, when its name stays.
fn moved_between(old: &str, new: &str) -> Option<(String, String)> {
    let (old_dir, old_name) = old.rsplit_once('/').unwrap_or(("", old));
    let (new_dir, new_name) = new.rsplit_once('/').unwrap_or(("", new));
    (old_name == new_name && old_dir != new_dir).then(|| (old_dir.to_owned(), new_dir.to_owned()))
}

/// The deleted file most of whose lines `added` has, if any reaches
/// [`LIKELY_RENAME`]. Only whole files are compared: both sides' hunks hold
/// every line of an added or deleted file.
fn likely_source<'a>(added: &FileDiff, deleted: &[&'a FileDiff]) -> Option<&'a str> {
    let lines = |hunks: &str, sign: char| -> Vec<String> {
        hunks
            .lines()
            .filter_map(|l| l.strip_prefix(sign))
            .map(str::trim)
            .filter(|l| l.len() > 3)
            .map(str::to_owned)
            .collect()
    };
    let new_lines = lines(&added.hunks, '+');
    if new_lines.is_empty() {
        return None;
    }
    let mut best: Option<(f64, &str)> = None;
    for old in deleted {
        let old_lines = lines(&old.hunks, '-');
        if old_lines.is_empty() {
            continue;
        }
        let shared = old_lines.iter().filter(|l| new_lines.contains(l)).count();
        let score = shared as f64 / old_lines.len().min(new_lines.len()) as f64;
        if score >= LIKELY_RENAME && best.is_none_or(|(s, _)| score > s) {
            best = Some((score, old.path.as_str()));
        }
    }
    best.map(|(_, path)| path)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, hunks: &str) -> FileDiff {
        let added = hunks.lines().filter(|l| l.starts_with('+')).count();
        let deleted = hunks.lines().filter(|l| l.starts_with('-')).count();
        FileDiff {
            path: path.to_owned(),
            old_path: None,
            lines: Some((added, deleted)),
            hunks: hunks.to_owned(),
        }
    }

    fn change(status: ChangeStatus, path: &str, old: Option<&str>) -> FileChange {
        FileChange {
            status,
            path: path.to_owned(),
            old_path: old.map(str::to_owned),
        }
    }

    #[test]
    fn whitespace_changes() {
        for (hunks, spacing) in [
            (
                "@@ -1,2 +1,2 @@\n-x = 1\n-y = 2\n+x = 1   \n+y = 2  \n",
                Spacing::TrailingSpaces,
            ),
            (
                "@@ -1,2 +1,2 @@\n-x = 1\r\n-y = 2\r\n+x = 1\n+y = 2\n",
                Spacing::LineEndings { to_lf: true },
            ),
            (
                "@@ -1 +1 @@\n-x = 1\n+x = 1\r\n",
                Spacing::LineEndings { to_lf: false },
            ),
            (
                "@@ -1 +1 @@\n-tabulate==0.9.0\n\\ No newline at end of file\n+tabulate==0.9.0\n",
                Spacing::FinalNewline,
            ),
            (
                "@@ -1,2 +1,2 @@\n-if x:\n-  go()\n+if x:\n+    go()\n",
                Spacing::Indentation,
            ),
            ("@@ -1,2 +1,4 @@\n a\n+\n+\n b\n", Spacing::BlankLines),
            (
                "@@ -1 +1,3 @@\n-f(a, b)\n+f(\n+    a, b\n+)\n",
                Spacing::LineBreaks,
            ),
        ] {
            assert_eq!(
                change_of(&file("a.py", hunks)),
                Change::Whitespace(spacing),
                "{hunks}"
            );
        }
        assert!(Spacing::LineBreaks.shows_in_diff());
        assert!(!Spacing::TrailingSpaces.shows_in_diff());
        assert_eq!(
            change_of(&file("a.py", "@@ -1 +1 @@\n-x = 1\n+x = 2\n")),
            Change::Content
        );
    }

    #[test]
    fn comment_changes() {
        let python = [
            "@@ -1,2 +1,3 @@\n def run():\n+    # read the input first\n     go()\n",
            "@@ -1,2 +1,3 @@\n def run():\n+    \"\"\"Main loop: reads, updates, draws.\"\"\"\n     go()\n",
            "@@ -1 +1,4 @@\n def run():\n+    \"\"\"Main loop.\n+\n+    Reads, updates, draws.\n+    \"\"\"\n",
        ];
        for hunks in python {
            assert_eq!(
                change_of(&file("game.py", hunks)),
                Change::Comments,
                "{hunks}"
            );
        }
        let ts = "@@ -1 +1,4 @@\n+/**\n+ * Sums cents.\n+ */\n export function sum() {}\n";
        assert_eq!(change_of(&file("money.ts", ts)), Change::Comments);
        assert_eq!(
            change_of(&file("money.ts", "@@ -1 +1 @@\n-// acount\n+// account\n")),
            Change::Comments
        );
        // Code next to a comment, or a language without known comments.
        let mixed = "@@ -1 +1,2 @@\n+# new limit\n+LIMIT = 10\n";
        assert_eq!(change_of(&file("a.py", mixed)), Change::Content);
        assert_eq!(
            change_of(&file("a.md", "@@ -1 +1 @@\n+# Title\n")),
            Change::Content
        );
        // A string that only looks like a comment start is code.
        assert_eq!(
            change_of(&file("a.py", "@@ -1 +1 @@\n-x = \"#1\"\n+x = \"#2\"\n")),
            Change::Content
        );
    }

    #[test]
    fn no_lines_and_binaries() {
        assert_eq!(change_of(&file("run.sh", "")), Change::NoLines);
        let binary = FileDiff {
            lines: None,
            ..file("logo.png", "")
        };
        assert_eq!(change_of(&binary), Change::Content);
    }

    #[test]
    fn rename_paths() {
        assert_eq!(
            rename_path("src/controls.py", "src/input.py"),
            "src/{controls.py => input.py}"
        );
        assert_eq!(
            rename_path("src/objects/chair.py", "src/models/chair.py"),
            "src/{objects => models}/chair.py"
        );
        assert_eq!(rename_path("a.rs", "b/a.rs"), "{a.rs => b/a.rs}");
        assert_eq!(
            rename_path("x/y/z.rs", "x/y/z.rs.bak"),
            "x/y/{z.rs => z.rs.bak}"
        );
    }

    #[test]
    fn statuses_in_the_list() {
        let files = [
            file("src/new.py", "@@ -0,0 +1,2 @@\n+a = 1\n+b = 2\n"),
            file("src/old.py", "@@ -1 +0,0 @@\n-gone = True\n"),
            file("src/py.typed", ""),
            file("scripts/run.sh", ""),
            FileDiff {
                old_path: Some("src/controls.py".into()),
                ..file("src/input.py", "")
            },
            file("src/state.py", "@@ -1 +1 @@\n-x = 1\n+x = 1  \n"),
            file(
                "src/game.py",
                "@@ -1 +1,2 @@\n def run():\n+    # main loop\n",
            ),
            file("src/cli.py", "@@ -1 +1 @@\n-x = 1\n+x = 2\n"),
        ];
        let staged = [
            change(ChangeStatus::Added, "src/new.py", None),
            change(ChangeStatus::Deleted, "src/old.py", None),
            change(ChangeStatus::Added, "src/py.typed", None),
            change(ChangeStatus::Modified, "scripts/run.sh", None),
            change(
                ChangeStatus::Renamed,
                "src/input.py",
                Some("src/controls.py"),
            ),
            change(ChangeStatus::Modified, "src/state.py", None),
            change(ChangeStatus::Modified, "src/game.py", None),
            change(ChangeStatus::Modified, "src/cli.py", None),
        ];
        assert_eq!(
            stat_lines(&files, &staged).concat(),
            "src/new.py | new, +2 -0\n\
             src/old.py | deleted, +0 -1\n\
             src/py.typed | new, empty, +0 -0\n\
             scripts/run.sh | no line changed (mode), +0 -0\n\
             src/{controls.py => input.py} | renamed, unchanged, +0 -0\n\
             src/state.py | whitespace only: trailing spaces, +1 -1\n\
             src/game.py | comments only, +1 -0\n\
             src/cli.py | +1 -1\n"
        );
    }

    #[test]
    fn renames_between_two_directories_take_one_line() {
        let mut files: Vec<FileDiff> = ["chair", "table", "lamp"]
            .iter()
            .map(|name| FileDiff {
                old_path: Some(format!("src/objects/{name}.py")),
                ..file(&format!("src/models/{name}.py"), "@@ -1 +1 @@\n-a\n+b\n")
            })
            .collect();
        files.insert(1, file("src/game.py", "@@ -1 +1 @@\n-a\n+b\n"));
        let staged: Vec<FileChange> = files
            .iter()
            .map(|f| match &f.old_path {
                Some(old) => change(ChangeStatus::Renamed, &f.path, Some(old)),
                None => change(ChangeStatus::Modified, &f.path, None),
            })
            .collect();
        assert_eq!(
            stat_lines(&files, &staged),
            [
                "src/{objects => models}/ | 3 files renamed, +3 -3\n",
                "src/game.py | +1 -1\n"
            ]
        );
        // Two are not a group.
        files.truncate(3);
        assert_eq!(stat_lines(&files, &staged[..3]).len(), 3);
    }

    #[test]
    fn a_deletion_like_a_new_file_is_named() {
        let old = "@@ -1,4 +0,0 @@\n-class Config:\n-    loja = \"Café\"\n-    moeda = \"R$\"\n-    taxa = 0.12\n";
        let new = "@@ -0,0 +1,6 @@\n+class Ajustes(NamedTuple):\n+    loja = \"Café\"\n+    moeda = \"R$\"\n+    taxa = 0.12\n+\n+PADRAO = Ajustes()\n";
        let files = [
            file("src/config.py", old),
            file("src/ajustes.py", new),
            file("src/other.py", "@@ -0,0 +1 @@\n+unrelated = 1\n"),
        ];
        let staged = [
            change(ChangeStatus::Deleted, "src/config.py", None),
            change(ChangeStatus::Added, "src/ajustes.py", None),
            change(ChangeStatus::Added, "src/other.py", None),
        ];
        let lines = stat_lines(&files, &staged);
        assert_eq!(
            lines[1],
            "src/ajustes.py | new, maybe renamed from src/config.py, +6 -0\n"
        );
        assert_eq!(lines[2], "src/other.py | new, +1 -0\n");
    }

    #[test]
    fn unknown_files_keep_the_plain_line() {
        // Recorded inputs without a staged entry for the file.
        let files = [file("src/a.rs", "@@ -1 +1 @@\n-a\n+b\n")];
        assert_eq!(stat_lines(&files, &[]), ["src/a.rs | +1 -1\n"]);
    }
}
