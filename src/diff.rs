//! Fits a staged diff into the small prompt an NPU model accepts.
//!
//! Files are classified by path, noise (lockfiles, generated and binary
//! files) is reduced to a stat line, and the remaining token budget is shared
//! between files by priority: source first, then config, tests and docs.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Rough characters per token for code and pt/en prose with Qwen-style
/// tokenizers. Deliberately pessimistic: overflowing the NPU's fixed prompt
/// length fails the generation, while underfilling only loses some context.
pub const CHARS_PER_TOKEN: usize = 3;

/// Below this, a file's hunks are dropped instead of truncated: a few lines
/// of a large change mislead more than they help.
const MIN_FILE_TOKENS: usize = 24;

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
        classify_path(path)
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

/// Parses `git diff --numstat -z` and the matching patch.
///
/// Both list files in the same order, so the n-th patch section belongs to
/// the n-th numstat entry; this avoids parsing (possibly quoted, possibly
/// prefix-less) paths out of the patch headers. If the two disagree, the
/// hunks are dropped and only the stat survives.
pub fn parse(numstat_z: &str, patch: &str) -> Vec<FileDiff> {
    let mut files = parse_numstat_z(numstat_z);
    let sections = split_sections(patch);
    if sections.len() == files.len() {
        for (file, section) in files.iter_mut().zip(sections) {
            file.hunks = hunks_of(section).to_owned();
        }
    }
    files
}

/// Parses `added\tdeleted\tpath\0`, or `added\tdeleted\t\0old\0new\0` for
/// renames and copies. Binary files have `-` counts.
fn parse_numstat_z(out: &str) -> Vec<FileDiff> {
    let mut fields = out.split('\0');
    let mut files = Vec::new();
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let lines = added.parse().ok().zip(deleted.parse().ok());
        let (path, old_path) = if path.is_empty() {
            let (Some(old), Some(new)) = (fields.next(), fields.next()) else {
                break;
            };
            (new, Some(old.to_owned()))
        } else {
            (path, None)
        };
        files.push(FileDiff {
            path: path.to_owned(),
            old_path,
            lines,
            hunks: String::new(),
        });
    }
    files
}

/// Splits a patch at each `diff --git` (or `diff --cc` for conflicts).
fn split_sections(patch: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = Vec::new();
    let mut offset = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --") {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| &patch[start..starts.get(i + 1).copied().unwrap_or(patch.len())])
        .collect()
}

fn hunks_of(section: &str) -> &str {
    if section.starts_with("@@") {
        return section;
    }
    section.find("\n@@").map_or("", |i| &section[i + 1..])
}

const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
    "flake.lock",
    "mix.lock",
    "Podfile.lock",
    "pubspec.lock",
    "packages.lock.json",
];
const GENERATED_DIRS: &[&str] = &[
    "dist",
    "build",
    "target",
    "vendor",
    "node_modules",
    "__pycache__",
    "generated",
];
const GENERATED_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.css",
    ".map",
    ".pb.go",
    "_pb2.py",
    ".g.dart",
    ".freezed.dart",
];
const TEST_DIRS: &[&str] = &["test", "tests", "__tests__", "spec", "specs", "testdata"];
const DOC_DIRS: &[&str] = &["doc", "docs"];
const DOC_EXTENSIONS: &[&str] = &["md", "mdx", "rst", "adoc", "txt"];
const DOC_STEMS: &[&str] = &["readme", "changelog", "license", "contributing", "authors"];
const CONFIG_EXTENSIONS: &[&str] = &["toml", "yaml", "yml", "json", "ini", "cfg", "conf", "env"];

fn classify_path(path: &str) -> FileKind {
    let mut components: Vec<&str> = path.split('/').collect();
    let name = components.pop().unwrap_or(path);
    let lower = name.to_lowercase();
    let (stem, extension) = lower.rsplit_once('.').unwrap_or((&lower, ""));
    let in_dir = |dirs: &[&str]| components.iter().any(|c| dirs.contains(c));

    if LOCKFILES.contains(&name) || extension == "lock" {
        FileKind::Lockfile
    } else if in_dir(GENERATED_DIRS)
        || GENERATED_SUFFIXES.iter().any(|s| lower.ends_with(s))
        || lower.contains(".generated.")
    {
        FileKind::Generated
    } else if in_dir(TEST_DIRS)
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
    {
        FileKind::Test
    } else if in_dir(DOC_DIRS) || DOC_EXTENSIONS.contains(&extension) || DOC_STEMS.contains(&stem) {
        FileKind::Docs
    } else if CONFIG_EXTENSIONS.contains(&extension)
        || name.starts_with('.')
        || matches!(lower.as_str(), "dockerfile" | "makefile")
    {
        FileKind::Config
    } else {
        FileKind::Source
    }
}

/// Fits `files` into `budget` tokens: the stat first, then hunks.
///
/// The stat is capped at half the budget; files beyond it are summarised in
/// one line. Hunks are shared by priority tier: each tier splits what is left
/// evenly, files needing less than their share give the rest back, and
/// whatever the tier does not use goes to the next one.
///
/// The only way to exceed `budget` is a budget too small for the stat's
/// summary line itself, which is always kept.
pub fn compact(files: &[FileDiff], budget: usize) -> CompactDiff {
    let stat = render_stat(files, budget / 2);
    let mut remaining = budget.saturating_sub(estimate_tokens(&stat));

    // (kind, tokens needed, index): sorting puts each tier's smallest
    // files first, so their leftovers flow to the larger ones.
    let mut order: Vec<(FileKind, usize, usize)> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.has_useful_hunks())
        .map(|(i, f)| (FileKind::of(f), estimate_tokens(&f.section()), i))
        .collect();
    order.sort();

    let mut allotted = vec![0usize; files.len()];
    for tier in order.chunk_by(|a, b| a.0 == b.0) {
        for (left, &(_, need, i)) in (1..=tier.len()).rev().zip(tier) {
            let grant = need.min(remaining / left);
            if grant == need || grant >= MIN_FILE_TOKENS {
                allotted[i] = grant;
                remaining -= grant;
            }
        }
    }

    let mut patch = String::new();
    let mut truncated = 0;
    for (file, &tokens) in files.iter().zip(&allotted) {
        if tokens == 0 {
            truncated += usize::from(file.has_useful_hunks());
            continue;
        }
        let (text, cut) = fit_file(file, tokens);
        truncated += usize::from(cut);
        patch.push_str(&text);
    }
    CompactDiff {
        stat,
        patch,
        truncated,
    }
}

fn render_stat(files: &[FileDiff], budget: usize) -> String {
    let lines: Vec<String> = files.iter().map(stat_line).collect();
    let total: usize = lines.iter().map(|l| estimate_tokens(l)).sum();
    if total <= budget {
        return lines.concat();
    }
    // Not everything fits: keep room for the summary of what is left out.
    let mut stat = String::new();
    let mut used = 0;
    for (shown, line) in lines.iter().enumerate() {
        let summary = format!("... and {} more files\n", lines.len() - shown);
        if used + estimate_tokens(line) + estimate_tokens(&summary) > budget {
            stat.push_str(&summary);
            break;
        }
        stat.push_str(line);
        used += estimate_tokens(line);
    }
    stat
}

fn stat_line(file: &FileDiff) -> String {
    let path = match &file.old_path {
        Some(old) => format!("{old} => {}", file.path),
        None => file.path.clone(),
    };
    match file.lines {
        Some((added, deleted)) => format!("{path} | +{added} -{deleted}\n"),
        None => format!("{path} | binary\n"),
    }
}

/// Keeps whole lines of the file's section while they fit in `tokens`.
fn fit_file(file: &FileDiff, tokens: usize) -> (String, bool) {
    let full = file.section();
    if estimate_tokens(&full) <= tokens {
        return (full, false);
    }
    let lines: Vec<&str> = full.lines().collect();
    let mut kept = String::new();
    // Sum of per-line estimates: an upper bound of the estimate of `kept`.
    let mut used = 0;
    for (n, line) in lines.iter().enumerate() {
        let marker = format!("[... {} more lines]\n", lines.len() - n);
        let cost = estimate_tokens(line) + 1;
        if used + cost + estimate_tokens(&marker) > tokens {
            kept.push_str(&marker);
            break;
        }
        let _ = writeln!(kept, "{line}");
        used += cost;
    }
    (kept, true)
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

    fn hunk(lines: usize) -> String {
        let mut text = format!("@@ -1,{lines} +1,{lines} @@\n");
        for n in 0..lines {
            let _ = writeln!(text, "+line {n} of a fairly ordinary change");
        }
        text
    }

    #[test]
    fn parses_numstat_with_renames_and_binaries() {
        let out = "3\t1\tsrc/lib.rs\0-\t-\tlogo.png\0\
                   0\t0\t\0old name.rs\0new name.rs\0";
        let files = parse_numstat_z(out);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].lines, Some((3, 1)));
        assert_eq!(files[1].lines, None);
        assert_eq!(files[2].path, "new name.rs");
        assert_eq!(files[2].old_path.as_deref(), Some("old name.rs"));
    }

    #[test]
    fn pairs_patch_sections_with_numstat_by_order() {
        let patch = "diff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n\
                     @@ -1 +1 @@\n-old\n+new\n\
                     diff --git a/logo.png b/logo.png\nBinary files differ\n";
        let files = parse("1\t1\ta.rs\0-\t-\tlogo.png\0", patch);
        assert_eq!(files[0].hunks, "@@ -1 +1 @@\n-old\n+new\n");
        assert_eq!(files[1].hunks, "");
    }

    #[test]
    fn conflict_sections_are_split_too() {
        let patch = "diff --cc both.rs\nindex 1,2..3\n@@@ -1,1 -1,1 +1,1 @@@\n- a\n -b\n++c\n\
                     diff --git a/x.rs b/x.rs\n@@ -1 +1 @@\n-x\n+y\n";
        let files = parse("1\t1\tboth.rs\x001\t1\tx.rs\0", patch);
        assert!(files[0].hunks.starts_with("@@@"));
        assert_eq!(files[1].hunks, "@@ -1 +1 @@\n-x\n+y\n");
    }

    #[test]
    fn lines_starting_with_diff_inside_hunks_do_not_split() {
        // A removed line `-diff --git` starts with `-`, not `diff`.
        let patch = "diff --git a/a.md b/a.md\n@@ -1 +1 @@\n-diff --git x\n+ok\n";
        let files = parse("1\t1\ta.md\0", patch);
        assert!(files[0].hunks.ends_with("-diff --git x\n+ok\n"));
    }

    #[test]
    fn last_stat_line_needs_no_room_for_the_summary() {
        let files = [file("a.rs", ""), file("b.rs", "")];
        let exact = estimate_tokens(&stat_line(&files[0])) + estimate_tokens(&stat_line(&files[1]));
        assert_eq!(render_stat(&files, exact), "a.rs | +0 -0\nb.rs | +0 -0\n");
        // One token short: a line plus the summary would not fit either.
        assert_eq!(render_stat(&files, exact - 1), "... and 2 more files\n");
    }

    #[test]
    fn mismatched_patch_keeps_only_the_stat() {
        let files = parse("1\t1\ta.rs\0", "");
        assert_eq!(files.len(), 1);
        assert!(files[0].hunks.is_empty());
    }

    #[test]
    fn classifies_paths() {
        for (path, kind) in [
            ("src/main.rs", FileKind::Source),
            ("lib/parser.py", FileKind::Source),
            ("Cargo.toml", FileKind::Config),
            (".github/workflows/ci.yml", FileKind::Config),
            ("Dockerfile", FileKind::Config),
            ("tests/git_context.rs", FileKind::Test),
            ("pkg/diff_test.go", FileKind::Test),
            ("web/app.spec.ts", FileKind::Test),
            ("test_style.py", FileKind::Test),
            ("README.md", FileKind::Docs),
            ("docs/01-design.md", FileKind::Docs),
            ("LICENSE", FileKind::Docs),
            ("Cargo.lock", FileKind::Lockfile),
            ("web/package-lock.json", FileKind::Lockfile),
            ("dist/app.js", FileKind::Generated),
            ("static/app.min.js", FileKind::Generated),
            ("api/v1/user.pb.go", FileKind::Generated),
        ] {
            assert_eq!(classify_path(path), kind, "{path}");
        }
    }

    #[test]
    fn small_diff_is_kept_whole() {
        let files = [file("src/a.rs", "@@ -1 +1 @@\n-old\n+new\n")];
        let diff = compact(&files, 200);
        assert_eq!(diff.stat, "src/a.rs | +1 -1\n");
        assert_eq!(diff.patch, "--- src/a.rs\n@@ -1 +1 @@\n-old\n+new\n");
        assert_eq!(diff.truncated, 0);
    }

    #[test]
    fn noise_appears_only_in_the_stat() {
        let files = [
            file("Cargo.lock", &hunk(5)),
            FileDiff {
                lines: None,
                ..file("logo.png", "")
            },
            file("src/a.rs", &hunk(1)),
        ];
        let diff = compact(&files, 500);
        assert!(diff.stat.contains("Cargo.lock | +5 -0"));
        assert!(diff.stat.contains("logo.png | binary"));
        assert!(!diff.patch.contains("Cargo.lock"));
        assert!(diff.patch.contains("--- src/a.rs"));
        assert_eq!(diff.truncated, 0, "noise is not counted as truncated");
    }

    #[test]
    fn source_takes_priority_over_docs() {
        let files = [file("README.md", &hunk(40)), file("src/a.rs", &hunk(40))];
        let diff = compact(&files, 300);
        assert!(diff.patch.contains("--- src/a.rs"));
        assert!(!diff.patch.contains("--- README.md"));
        assert_eq!(diff.truncated, 2);
    }

    #[test]
    fn leftovers_from_small_files_go_to_large_ones() {
        let files = [
            file("src/big.rs", &hunk(100)),
            file("src/small.rs", &hunk(1)),
        ];
        let diff = compact(&files, 400);
        let small = files[1].section();
        assert!(diff.patch.contains(&small), "the small file is whole");
        assert!(diff.patch.contains("--- src/big.rs"));
        assert!(diff.patch.contains("more lines]"));
        assert_eq!(diff.truncated, 1);
    }

    #[test]
    fn stays_within_budget() {
        let files: Vec<FileDiff> = (0..30)
            .map(|n| file(&format!("src/m{n}.rs"), &hunk(n + 1)))
            .collect();
        for budget in [50, 200, 700, 2000] {
            let diff = compact(&files, budget);
            let used = estimate_tokens(&diff.stat) + estimate_tokens(&diff.patch);
            assert!(used <= budget, "{used} > {budget}");
        }
    }

    #[test]
    fn long_stat_is_summarised() {
        let files: Vec<FileDiff> = (0..200)
            .map(|n| file(&format!("src/module_{n}.rs"), ""))
            .collect();
        let diff = compact(&files, 200);
        assert!(estimate_tokens(&diff.stat) <= 100);
        assert!(diff.stat.ends_with("more files\n"), "{}", diff.stat);
    }
}
