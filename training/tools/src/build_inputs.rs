//! CommitChronicle parquet shards -> smart-commit trace `Inputs` + targets.
//!
//! Writes two aligned JSON Lines files: `inputs.jsonl` (one `trace::Inputs`
//! per line, for `smart-commit prompt --inputs`) and `meta.jsonl`
//! (repository, hash, target subject and its Conventional Commits type). The
//! prompt itself is built by smart-commit, never here.
//!
//! Deterministic: the same shards give byte-identical outputs.
//!
//! With `--releases` it keeps, instead, only the release bumps the target
//! filters drop whose version is on an added line of the diff, where the
//! subject can be told from the change (H7).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context as _, Result, bail};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use parquet::schema::types::Type;
use regex::Regex;
use serde_json::{Map, Value, json};

use crate::pyjson;
use crate::pytext;

pub const HISTORY_DEPTH: usize = 30; // style.history_depth
pub const SUBJECT_MIN: usize = 8;
pub const SUBJECT_MAX: usize = 72;
const CONTEXT: usize = 1; // smart-commit runs `git diff -U1`
const MAX_FILES: usize = 64;
const MAX_DIFF_BYTES: usize = 64 * 1024; // per file; the prompt keeps far less
const HISTORY_FILES: usize = 20; // paths kept per history commit, like git::Repo::recent_commits

const TYPES: &str = "feat|fix|docs|style|refactor|perf|test|tests|build|ci|chore|revert";

/// Spellings of one type, folded for selection and scoring; the target keeps
/// the repository's own spelling, which the model should follow.
pub fn fold_type(kind: &str) -> &str {
    if kind == "tests" { "test" } else { kind }
}

fn insensitive(patterns: &[&str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|p| Regex::new(&format!("(?i){p}")).expect("a valid pattern"))
        .collect()
}

static CONVENTIONAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?P<type>{TYPES})(\([^()]+\))?!?: \S")).expect("a valid pattern")
});

const RELEASE_PATTERNS: [&str; 4] = [
    r"^(chore(\([^)]*\))?: )?(release|bump version|version bump|prepare release)\b",
    r"^v?\d+(\.\d+)+\S*$",
    // A description that is only a version: "chore(release): 0.3.8".
    r"^\S+?(\([^)]*\))?!?:\s*(release\s+|version\s+)?v?\d+(\.\d+)+\S*$",
    r"^(release|version|bump)\s+v?\d+(\.\d+)+\S*$",
];

static RELEASE: LazyLock<Vec<Regex>> = LazyLock::new(|| insensitive(&RELEASE_PATTERNS));

static DROP: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let mut patterns = vec![
        r"^merge\b",
        r"^revert\b",
        r"^(fixup|squash|amend)!",
        r"\bwip\b",
    ];
    patterns.extend(RELEASE_PATTERNS);
    patterns.extend([
        r"^(\W*(#|gh-)\d+\W*)+$",
        r"^(update|fix|close[sd]?|resolve[sd]?)\s+#\d+$",
    ]);
    insensitive(&patterns)
});

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bv?(\d+(?:\.\d+)+(?:[-+][\w.]+)?)").expect("a valid pattern"));

static HUNK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$").expect("a valid pattern")
});

pub fn subject_of(message: &str) -> &str {
    let message = pytext::strip(message);
    pytext::strip(message.split('\n').next().unwrap_or(""))
}

pub fn keep_target(subject: &str) -> bool {
    let length = subject.chars().count();
    (SUBJECT_MIN..=SUBJECT_MAX).contains(&length) && !DROP.iter().any(|p| p.is_match(subject))
}

/// The version a release bump names ("chore: release 1.2.0" -> "1.2.0"), or
/// None for any other subject, or one without a version.
pub fn release_version(subject: &str) -> Option<String> {
    if !RELEASE.iter().any(|p| p.is_match(subject)) {
        return None;
    }
    VERSION.captures(subject).map(|m| m[1].to_owned())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `version` is on an added line of the diff `inputs_of` gave, not
/// inside a longer version or word.
pub fn version_in_diff(version: &str, changes: &Changes) -> bool {
    changes
        .files
        .iter()
        .flat_map(|f| pytext::splitlines(&f.hunks))
        .filter(|line| line.starts_with('+'))
        .any(|line| {
            line.match_indices(version).any(|(at, _)| {
                let before = line[..at].chars().next_back();
                let after = line[at + version.len()..].chars().next();
                !before.is_some_and(|c| is_word(c) || c == '.') && !after.is_some_and(is_word)
            }) || overlapping(line, version)
        })
}

/// Occurrences that overlap another one, which `match_indices` skips.
fn overlapping(line: &str, version: &str) -> bool {
    line.char_indices().any(|(at, _)| {
        line[at..].starts_with(version) && {
            let before = line[..at].chars().next_back();
            let after = line[at + version.len()..].chars().next();
            !before.is_some_and(|c| is_word(c) || c == '.') && !after.is_some_and(is_word)
        }
    })
}

pub fn conventional_type(subject: &str) -> Option<String> {
    CONVENTIONAL
        .captures(subject)
        .map(|m| fold_type(&m["type"]).to_owned())
}

/// `%d.%m.%Y %H:%M:%S` as a sortable tuple.
pub fn date_key(date: &str) -> Result<(u32, u32, u32, u32, u32, u32)> {
    let bad = || format!("time data {date:?} does not match '%d.%m.%Y %H:%M:%S'");
    let (day, rest) = date.split_once('.').with_context(bad)?;
    let (month, rest) = rest.split_once('.').with_context(bad)?;
    let (year, time) = rest.split_once(' ').with_context(bad)?;
    let mut clock = time.split(':');
    let number =
        |text: Option<&str>| -> Result<u32> { text.and_then(|t| t.parse().ok()).with_context(bad) };
    let (d, mo, y) = (
        number(Some(day))?,
        number(Some(month))?,
        number(Some(year))?,
    );
    let (h, mi, s) = (
        number(clock.next())?,
        number(clock.next())?,
        number(clock.next())?,
    );
    Ok((y, mo, d, h, mi, s))
}

fn range_header(start: i64, count: usize) -> String {
    // git: "-a" when one line, "-a,0" with a = the line before when none.
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

/// Re-hunks a unified diff to `context` lines of context, as `-U1` would.
///
/// Hunks split where more than 2 * context unchanged lines separate two
/// changes. Only the first piece of a split hunk keeps git's function
/// heading, since the others' headings cannot be computed here.
pub fn trim_context(diff: &str, context: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut lines: Vec<&str> = diff.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut i = 0;
    while i < lines.len() {
        let Some(m) = HUNK.captures(lines[i]) else {
            i += 1;
            continue;
        };
        // An empty range names the line before it.
        let old: i64 =
            m[1].parse().unwrap_or(0) + i64::from(m.get(2).is_some_and(|c| c.as_str() == "0"));
        let new: i64 =
            m[3].parse().unwrap_or(0) + i64::from(m.get(4).is_some_and(|c| c.as_str() == "0"));
        let heading = m[5].to_owned();
        let mut body = Vec::new();
        i += 1;
        while i < lines.len() && !lines[i].starts_with("@@ ") {
            body.push(lines[i]);
            i += 1;
        }
        // Attach "\ No newline at end of file" to the line before it.
        let mut entries: Vec<Vec<&str>> = Vec::new();
        for line in body {
            if line.starts_with('\\') && !entries.is_empty() {
                entries.last_mut().expect("not empty").push(line);
            } else if line.starts_with([' ', '-', '+']) {
                entries.push(vec![line]);
            } else if line.is_empty() {
                entries.push(vec![" "]); // an empty context line lost its space
            }
        }
        let kind = |e: &Vec<&str>| e[0].as_bytes()[0];
        let changed: Vec<bool> = entries.iter().map(|e| kind(e) != b' ').collect();
        if !changed.iter().any(|&c| c) {
            continue;
        }
        let mut keep = vec![false; entries.len()];
        for (j, &c) in changed.iter().enumerate() {
            if c {
                for slot in
                    &mut keep[j.saturating_sub(context)..(j + context + 1).min(entries.len())]
                {
                    *slot = true;
                }
            }
        }
        // Line numbers before each entry.
        let (mut o, mut n) = (old, new);
        let mut pos = Vec::new();
        for e in &entries {
            pos.push((o, n));
            if kind(e) != b'+' {
                o += 1;
            }
            if kind(e) != b'-' {
                n += 1;
            }
        }
        let mut first = true;
        let mut j = 0;
        while j < entries.len() {
            if !keep[j] {
                j += 1;
                continue;
            }
            let start = j;
            while j < entries.len() && keep[j] {
                j += 1;
            }
            let piece = &entries[start..j];
            let (o0, n0) = pos[start];
            let oc = piece.iter().filter(|e| kind(e) != b'+').count();
            let nc = piece.iter().filter(|e| kind(e) != b'-').count();
            let ostart = if oc > 0 { o0 } else { o0 - 1 };
            let nstart = if nc > 0 { n0 } else { n0 - 1 };
            out.push(format!(
                "@@ -{} +{} @@{}",
                range_header(ostart, oc),
                range_header(nstart, nc),
                if first { heading.as_str() } else { "" }
            ));
            first = false;
            for e in piece {
                out.extend(e.iter().map(|l| (*l).to_owned()));
            }
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        out.join("\n") + "\n"
    }
}

fn line_counts(diff: &str) -> (usize, usize) {
    let (mut added, mut deleted) = (0, 0);
    for line in diff.split('\n') {
        if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('-') {
            deleted += 1;
        }
    }
    (added, deleted)
}

/// One file change of a commit, as the dataset lists it.
#[derive(Debug, Clone, Default)]
pub struct Mod {
    pub change_type: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub diff: Option<String>,
}

/// The paths a commit touched, as `git log --name-only` lists them (the old
/// path of deletions, the new one otherwise), sorted, at most HISTORY_FILES.
pub fn paths_of(mods: &[Mod]) -> Vec<String> {
    let mut paths: Vec<String> = mods
        .iter()
        .filter_map(|m| {
            if m.change_type == "DELETE" {
                m.old_path.clone()
            } else {
                m.new_path.clone()
            }
        })
        .filter(|p| !p.is_empty())
        .collect();
    paths.sort();
    paths.dedup();
    paths.truncate(HISTORY_FILES);
    paths
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub status: &'static str,
    pub path: String,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub lines: Option<(usize, usize)>,
    pub hunks: String,
}

/// `trace::Inputs` without branch and subjects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub staged: Vec<Staged>,
    pub files: Vec<FileDiff>,
}

fn status_of(change_type: &str) -> Option<&'static str> {
    Some(match change_type {
        "ADD" => "added",
        "MODIFY" => "modified",
        "DELETE" => "deleted",
        "RENAME" => "renamed",
        "COPY" => "copied",
        _ => return None,
    })
}

fn optional(text: &Option<String>) -> Value {
    text.as_ref().map_or(Value::Null, |t| json!(t))
}

impl Changes {
    pub fn staged_json(&self) -> Value {
        Value::Array(
            self.staged
                .iter()
                .map(|s| json!({"status": s.status, "path": s.path, "old_path": optional(&s.old_path)}))
                .collect(),
        )
    }

    pub fn files_json(&self) -> Value {
        Value::Array(
            self.files
                .iter()
                .map(|f| {
                    json!({
                        "path": f.path,
                        "old_path": optional(&f.old_path),
                        "lines": f.lines.map_or(Value::Null, |(a, d)| json!([a, d])),
                        "hunks": f.hunks,
                    })
                })
                .collect(),
        )
    }
}

/// `trace::Inputs` without branch and subjects, or None to skip.
pub fn inputs_of(mods: &[Mod]) -> Option<Changes> {
    if mods.is_empty() || mods.len() > MAX_FILES {
        return None;
    }
    let key = |m: &Mod| {
        m.new_path
            .clone()
            .filter(|p| !p.is_empty())
            .or_else(|| m.old_path.clone().filter(|p| !p.is_empty()))
            .unwrap_or_default()
    };
    let mut sorted: Vec<&Mod> = mods.iter().collect();
    sorted.sort_by_key(|m| key(m));
    let (mut staged, mut files) = (Vec::new(), Vec::new());
    for m in sorted {
        let status = status_of(&m.change_type)?;
        let path = if status == "deleted" {
            &m.old_path
        } else {
            &m.new_path
        };
        let path = path.clone().filter(|p| !p.is_empty())?;
        let old_path = if matches!(status, "renamed" | "copied") {
            m.old_path.clone()
        } else {
            None
        };
        let diff = m.diff.clone().unwrap_or_default();
        if diff.len() > MAX_DIFF_BYTES {
            return None;
        }
        let binary = diff.starts_with("Binary files")
            || (diff.is_empty() && matches!(status, "added" | "modified"));
        let hunks = if binary {
            String::new()
        } else {
            trim_context(&diff, CONTEXT)
        };
        staged.push(Staged {
            status,
            path: path.clone(),
            old_path: old_path.clone(),
        });
        files.push(FileDiff {
            path,
            old_path,
            lines: (!binary).then(|| line_counts(&hunks)),
            hunks,
        });
    }
    Some(Changes { staged, files })
}

#[derive(Debug, Default)]
pub struct Stats {
    pub commits: usize,
    pub dropped_target: usize,
    pub dropped_mods: usize,
    pub kept: usize,
}

struct Commit {
    repo: String,
    hash: String,
    date: String,
    message: String,
    mods: Vec<Mod>,
}

const COLUMNS: [&str; 5] = ["repo", "hash", "date", "message", "mods"];

fn text(field: &Field) -> Option<String> {
    match field {
        Field::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn mod_of(field: &Field) -> Mod {
    let mut m = Mod::default();
    if let Field::Group(row) = field {
        for (name, value) in row.get_column_iter() {
            match name.as_str() {
                "change_type" => m.change_type = text(value).unwrap_or_default(),
                "old_path" => m.old_path = text(value),
                "new_path" => m.new_path = text(value),
                "diff" => m.diff = text(value),
                _ => {}
            }
        }
    }
    m
}

/// Calls `each` with every commit of a shard, in its order.
fn read_shard(path: &Path, mut each: impl FnMut(Commit) -> Result<()>) -> Result<()> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let schema = reader.metadata().file_metadata().schema();
    let fields = schema
        .get_fields()
        .iter()
        .filter(|f| COLUMNS.contains(&f.name()))
        .cloned()
        .collect();
    let projection = Type::group_type_builder("schema")
        .with_fields(fields)
        .build()?;
    for row in reader.get_row_iter(Some(projection))? {
        let row = row?;
        let mut commit = Commit {
            repo: String::new(),
            hash: String::new(),
            date: String::new(),
            message: String::new(),
            mods: Vec::new(),
        };
        for (name, value) in row.get_column_iter() {
            match name.as_str() {
                "repo" => commit.repo = text(value).unwrap_or_default(),
                "hash" => commit.hash = text(value).unwrap_or_default(),
                "date" => commit.date = text(value).unwrap_or_default(),
                "message" => commit.message = text(value).unwrap_or_default(),
                "mods" => {
                    if let Field::ListInternal(list) = value {
                        commit.mods = list.elements().iter().map(mod_of).collect();
                    }
                }
                _ => {}
            }
        }
        each(commit)?;
    }
    Ok(())
}

pub struct Options {
    pub out: PathBuf,
    pub shards: Vec<PathBuf>,
    pub branch: Option<String>,
    pub releases: bool,
}

/// The commits of one repository, in date order, written when it is whole.
struct Writer<'a> {
    options: &'a Options,
    inputs: BufWriter<File>,
    meta: BufWriter<File>,
    stats: Stats,
}

impl Writer<'_> {
    fn repository(&mut self, mut rows: Vec<Commit>) -> Result<()> {
        let mut keyed = Vec::new();
        for row in rows.drain(..) {
            keyed.push((date_key(&row.date)?, row));
        }
        // Stable: commits with the same date keep the shard's order.
        keyed.sort_by_key(|(key, _)| *key);
        let rows: Vec<Commit> = keyed.into_iter().map(|(_, row)| row).collect();
        let subjects: Vec<&str> = rows.iter().map(|r| subject_of(&r.message)).collect();
        let touched: Vec<Vec<String>> = rows.iter().map(|r| paths_of(&r.mods)).collect();
        for (i, row) in rows.iter().enumerate() {
            self.stats.commits += 1;
            let target = subjects[i];
            let (version, keep) = if self.options.releases {
                let version = release_version(target);
                let length = target.chars().count();
                let keep = version.is_some() && (SUBJECT_MIN..=SUBJECT_MAX).contains(&length);
                (version, keep)
            } else {
                (None, keep_target(target))
            };
            if !keep {
                self.stats.dropped_target += 1;
                continue;
            }
            let Some(changes) = inputs_of(&row.mods) else {
                self.stats.dropped_mods += 1;
                continue;
            };
            if let Some(version) = &version
                && !version_in_diff(version, &changes)
            {
                self.stats.dropped_target += 1;
                continue;
            }
            let first = i.saturating_sub(HISTORY_DEPTH);
            let history: Vec<&str> = subjects[first..i].iter().rev().copied().collect();
            let history_files: Vec<&Vec<String>> = touched[first..i].iter().rev().collect();
            let mut inputs = Map::new();
            inputs.insert(
                "branch".into(),
                self.options
                    .branch
                    .as_ref()
                    .map_or(Value::Null, |b| json!(b)),
            );
            inputs.insert("subjects".into(), json!(history));
            inputs.insert("history_files".into(), json!(history_files));
            inputs.insert("staged".into(), changes.staged_json());
            inputs.insert("files".into(), changes.files_json());
            let meta = json!({
                "repo": row.repo,
                "hash": row.hash,
                "target": target,
                "type": conventional_type(target),
                "history": history.len(),
            });
            writeln!(self.inputs, "{}", pyjson::dumps(&Value::Object(inputs)))?;
            writeln!(self.meta, "{}", pyjson::dumps(&meta))?;
            self.stats.kept += 1;
        }
        Ok(())
    }
}

pub fn build(options: &Options) -> Result<Stats> {
    std::fs::create_dir_all(&options.out)?;
    let create = |name: &str| -> Result<BufWriter<File>> {
        Ok(BufWriter::new(File::create(options.out.join(name))?))
    };
    let mut writer = Writer {
        options,
        inputs: create("inputs.jsonl")?,
        meta: create("meta.jsonl")?,
        stats: Stats::default(),
    };
    for shard in &options.shards {
        let mut run: Vec<Commit> = Vec::new();
        read_shard(shard, |commit| {
            if run.first().is_some_and(|first| first.repo != commit.repo) {
                writer.repository(std::mem::take(&mut run))?;
            }
            run.push(commit);
            Ok(())
        })?;
        if !run.is_empty() {
            writer.repository(run)?;
        }
    }
    writer.inputs.flush()?;
    writer.meta.flush()?;
    if writer.stats.commits == 0 && options.shards.is_empty() {
        bail!("no shards");
    }
    Ok(writer.stats)
}

impl Stats {
    /// `json.dump(stats)`, as the Python tool printed it.
    pub fn json(&self) -> String {
        pyjson::dumps(&json!({
            "commits": self.commits,
            "dropped_target": self.dropped_target,
            "dropped_mods": self.dropped_mods,
            "kept": self.kept,
        }))
    }
}

#[cfg(test)]
#[path = "build_inputs_tests.rs"]
mod tests;
