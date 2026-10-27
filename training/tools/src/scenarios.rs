//! Commit scenarios: git operations on small synthetic repositories, each
//! with a hand-written target message.
//!
//! A scenario is data, not code. `scenarios/repos/<name>/` holds a base
//! repository (`files/`, and `repo.toml`, the history that introduces them);
//! `scenarios/cases/*.toml` hold the cases, each a list of operations from
//! `OPS` applied on top of a base repository. Building a case runs
//! smart-commit's mock backend on the result and keeps the trace's inputs, so
//! the cases come out in the format `select` writes (`inputs.jsonl` and
//! `meta.jsonl`): `make-examples` turns them into training examples, and
//! `smart-commit eval` and `score` evaluate them, unchanged.
//!
//! Everything is deterministic: fixed author, dates and git config, sorted
//! globs, seeded binary content. Building twice gives the same bytes.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use fancy_regex::Regex;
use serde_json::{Map, Value, json};
use toml::{Table, Value as Toml};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

use crate::mt::Mt;

pub const SPLITS: [&str; 2] = ["train", "eval"];
/// What the trace inputs do not record (the state of a merge or revert, file
/// modes): a target naming it cannot be learnt from the inputs, so these
/// cases measure only and never train.
pub const EVAL_ONLY_TAGS: [&str; 2] = ["git-state", "file-mode"];
const BASE_DATE: u64 = 1_767_268_800; // 2026-01-01T12:00:00Z

static VAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{([a-z_][a-z0-9_]*)\}\}").expect("a valid pattern"));
static COMMIT_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{commit:([^}]+)\}\}").expect("a valid pattern"));
static CONVENTIONAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z]+)(?:\([^()]*\))?!?: ").expect("a valid pattern"));

pub type Result<T> = std::result::Result<T, ScenarioError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioError(pub String);

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScenarioError {}

fn error<T>(text: impl Into<String>) -> Result<T> {
    Err(ScenarioError(text.into()))
}

impl From<std::io::Error> for ScenarioError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

/// Python's `repr` of a string, for messages.
fn repr(text: &str) -> String {
    if text.contains('\'') && !text.contains('"') {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('\n', "\\n"))
    } else {
        format!(
            "'{}'",
            text.replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\n', "\\n")
        )
    }
}

#[derive(Debug, Clone)]
pub struct Repo {
    pub name: String,
    pub convention: String,
    pub commits: Vec<Table>,
    pub files: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub title: String,
    pub repo: String,
    pub level: i64,
    pub split: String,
    pub target: String,
    pub ops: Vec<Table>,
    pub expect: Vec<String>,
    pub tags: Vec<String>,
    pub stage: String,
    pub history: bool,
    pub outcome: String,
    pub issue: String,
}

impl Default for Case {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            repo: String::new(),
            level: 0,
            split: String::new(),
            target: String::new(),
            ops: Vec::new(),
            expect: Vec::new(),
            tags: Vec::new(),
            stage: "all".into(),
            history: true,
            outcome: "message".into(),
            issue: String::new(),
        }
    }
}

// ------------------------------------------------------------------ loading

pub fn load_repos(root: &Path) -> Result<Vec<Repo>> {
    let mut specs: Vec<PathBuf> = glob_files(&root.join("repos/*/repo.toml"));
    specs.sort();
    let mut repos = Vec::new();
    for spec in specs {
        let data: Table = std::fs::read_to_string(&spec)?
            .parse()
            .map_err(|e| ScenarioError(format!("{}: {e}", spec.display())))?;
        let convention = data.get("convention").and_then(Toml::as_str);
        let Some(convention) =
            convention.filter(|c| ["free", "conventional", "gitmoji"].contains(c))
        else {
            return error(format!(
                "{}: convention must be free, conventional or gitmoji",
                spec.display()
            ));
        };
        let dir = spec.parent().expect("a file has a parent");
        repos.push(Repo {
            name: dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            convention: convention.to_owned(),
            commits: data
                .get("commit")
                .and_then(Toml::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_table().cloned()).collect())
                .unwrap_or_default(),
            files: dir.join("files"),
        });
    }
    Ok(repos)
}

fn glob_files(pattern: &Path) -> Vec<PathBuf> {
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    glob::glob_with(&pattern.to_string_lossy(), options)
        .map(|paths| paths.flatten().collect())
        .unwrap_or_default()
}

/// Python's `str(value)` of a variable.
fn display(value: &Toml) -> String {
    match value {
        Toml::String(s) => s.clone(),
        Toml::Integer(n) => n.to_string(),
        Toml::Float(f) => crate::pyjson::float(*f),
        Toml::Boolean(b) => if *b { "True" } else { "False" }.to_owned(),
        other => other.to_string(),
    }
}

/// Replaces `{{name}}` in every string of `value` (nested lists and tables
/// too); `{{commit:...}}` is left for `apply`, which resolves it.
pub fn substitute(value: &Toml, variables: &Table) -> Result<Toml> {
    Ok(match value {
        Toml::String(text) => {
            let mut out = String::new();
            let mut last = 0;
            for found in VAR.captures_iter(text.as_str()) {
                let found = found.map_err(|e| ScenarioError(e.to_string()))?;
                let whole = found.get(0).expect("group 0");
                out.push_str(&text[last..whole.start()]);
                let name = &found[1];
                match variables.get(name) {
                    Some(v) => out.push_str(&display(v)),
                    None => return error(format!("unknown variable {{{{{name}}}}}")),
                }
                last = whole.end();
            }
            out.push_str(&text[last..]);
            Toml::String(out)
        }
        Toml::Array(items) => Toml::Array(
            items
                .iter()
                .map(|v| substitute(v, variables))
                .collect::<Result<_>>()?,
        ),
        Toml::Table(table) => Toml::Table(
            table
                .iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, variables)?)))
                .collect::<Result<_>>()?,
        ),
        other => other.clone(),
    })
}

fn strings(value: Option<&Toml>) -> Vec<String> {
    value
        .and_then(Toml::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

impl Case {
    fn from_table(fields: &Table) -> std::result::Result<Case, String> {
        const KNOWN: [&str; 13] = [
            "id", "title", "repo", "level", "split", "target", "ops", "expect", "tags", "stage",
            "history", "outcome", "issue",
        ];
        if let Some(unknown) = fields.keys().find(|k| !KNOWN.contains(&k.as_str())) {
            return Err(format!("unexpected keyword argument {}", repr(unknown)));
        }
        let text = |key: &str| fields.get(key).and_then(Toml::as_str).map(str::to_owned);
        let required = |key: &str| text(key).ok_or_else(|| format!("missing {}", repr(key)));
        let default = Case::default();
        Ok(Case {
            id: required("id")?,
            title: required("title")?,
            repo: required("repo")?,
            level: fields
                .get("level")
                .and_then(Toml::as_integer)
                .ok_or("missing 'level'")?,
            split: required("split")?,
            target: required("target")?,
            ops: fields
                .get("ops")
                .and_then(Toml::as_array)
                .ok_or("missing 'ops'")?
                .iter()
                .filter_map(|o| o.as_table().cloned())
                .collect(),
            expect: strings(fields.get("expect")),
            tags: strings(fields.get("tags")),
            stage: text("stage").unwrap_or(default.stage),
            history: fields
                .get("history")
                .and_then(Toml::as_bool)
                .unwrap_or(default.history),
            outcome: text("outcome").unwrap_or(default.outcome),
            issue: text("issue").unwrap_or_default(),
        })
    }
}

/// One case, or one per variant: a variant is a set of variables, and may
/// override the split, the target and the expected words.
pub fn expand(raw: &Table, defaults: &Table) -> Result<Vec<Case>> {
    let mut base = defaults.clone();
    base.extend(raw.clone());
    let variants: Vec<Table> = match base.remove("variants") {
        Some(Toml::Array(v)) if !v.is_empty() => v
            .into_iter()
            .filter_map(|v| v.as_table().cloned())
            .collect(),
        _ => vec![Table::new()],
    };
    let many = variants.len() > 1;
    let mut cases = Vec::new();
    for (n, variant) in variants.iter().enumerate() {
        let variables = variant
            .get("vars")
            .and_then(Toml::as_table)
            .cloned()
            .unwrap_or_default();
        let mut fields = base.clone();
        fields.extend(
            variant
                .iter()
                .filter(|(k, _)| *k != "vars")
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        if many {
            let name = match variant.get("name") {
                Some(name) => display(name),
                None => (n + 1).to_string(),
            };
            let id = base.get("id").and_then(Toml::as_str).unwrap_or_default();
            fields.insert("id".into(), Toml::String(format!("{id}/{name}")));
        }
        fields.remove("name");
        if !variables.is_empty() {
            let Toml::Table(substituted) = substitute(&Toml::Table(fields), &variables)? else {
                unreachable!("a table stays a table")
            };
            fields = substituted;
        }
        let id = fields
            .get("id")
            .and_then(Toml::as_str)
            .unwrap_or("None")
            .to_owned();
        cases
            .push(Case::from_table(&fields).map_err(|e| ScenarioError(format!("case {id}: {e}")))?);
    }
    Ok(cases)
}

pub fn load_cases(root: &Path, repos: &[Repo]) -> Result<Vec<Case>> {
    let mut paths = glob_files(&root.join("cases/*.toml"));
    paths.sort();
    let mut cases = Vec::new();
    for path in paths {
        let data: Table = std::fs::read_to_string(&path)?
            .parse()
            .map_err(|e| ScenarioError(format!("{}: {e}", path.display())))?;
        let defaults: Table = ["level", "repo", "split"]
            .iter()
            .filter_map(|k| data.get(*k).map(|v| ((*k).to_owned(), v.clone())))
            .collect();
        for raw in data
            .get("case")
            .and_then(Toml::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(raw) = raw.as_table() {
                cases.extend(expand(raw, &defaults)?);
            }
        }
    }
    validate(&cases, repos)?;
    Ok(cases)
}

pub const OPS: [&str; 11] = [
    "write", "append", "prepend", "replace", "regex", "remove", "move", "chmod", "symlink",
    "binary", "git",
];

pub fn validate(cases: &[Case], repos: &[Repo]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for case in cases {
        let at = format!("case {}", case.id);
        if !seen.insert(case.id.clone()) {
            return error(format!("{at}: duplicate id"));
        }
        if !repos.iter().any(|r| r.name == case.repo) {
            return error(format!("{at}: unknown repo {}", case.repo));
        }
        if !SPLITS.contains(&case.split.as_str()) {
            return error(format!("{at}: split must be one of {SPLITS:?}"));
        }
        let eval_only: Vec<&str> = EVAL_ONLY_TAGS
            .iter()
            .copied()
            .filter(|t| case.tags.iter().any(|c| c == t))
            .collect();
        if case.split == "train" && !eval_only.is_empty() {
            return error(format!("{at}: tags {eval_only:?} are eval only"));
        }
        if !["message", "error"].contains(&case.outcome.as_str()) {
            return error(format!("{at}: outcome must be message or error"));
        }
        if case.outcome == "message" && case.target.is_empty() {
            return error(format!("{at}: no target"));
        }
        if !["all", "none"].contains(&case.stage.as_str()) {
            return error(format!("{at}: stage must be all or none"));
        }
        for op in &case.ops {
            let name = op.get("op").and_then(Toml::as_str);
            if !name.is_some_and(|n| OPS.contains(&n)) {
                return error(format!(
                    "{at}: unknown op {}",
                    name.map_or("None".into(), repr)
                ));
            }
        }
        for pattern in &case.expect {
            Regex::new(pattern).map_err(|e| ScenarioError(format!("{at}: {e}")))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- git

/// git in `cwd`, isolated from the machine's configuration, with a fixed
/// identity and a clock that advances one minute per commit.
#[derive(Debug)]
pub struct Git {
    pub cwd: PathBuf,
    ticks: Cell<u64>,
}

pub fn git_env(cwd: &Path, ticks: u64) -> Vec<(String, String)> {
    let date = format!("@{} +0000", BASE_DATE + 60 * ticks);
    let var = |k: &str, v: String| (k.to_owned(), v);
    vec![
        var(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        ),
        var("HOME", cwd.to_string_lossy().into_owned()),
        var("LANG", "C.UTF-8".into()),
        var("GIT_CONFIG_GLOBAL", "/dev/null".into()),
        var("GIT_CONFIG_NOSYSTEM", "1".into()),
        var("GIT_AUTHOR_NAME", "Scenario".into()),
        var("GIT_AUTHOR_EMAIL", "scenario@example.com".into()),
        var("GIT_COMMITTER_NAME", "Scenario".into()),
        var("GIT_COMMITTER_EMAIL", "scenario@example.com".into()),
        var("GIT_AUTHOR_DATE", date.clone()),
        var("GIT_COMMITTER_DATE", date),
    ]
}

impl Git {
    pub fn new(cwd: &Path) -> Self {
        Self {
            cwd: cwd.to_owned(),
            ticks: Cell::new(0),
        }
    }

    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.call(args, true)
    }

    pub fn call(&self, args: &[&str], check: bool) -> Result<String> {
        self.ticks.set(self.ticks.get() + 1);
        let output = Command::new("git")
            .args([
                "-c",
                "core.autocrlf=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(git_env(&self.cwd, self.ticks.get()))
            .output()?;
        if check && !output.status.success() {
            return error(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    let mut entries: Vec<_> = std::fs::read_dir(from)?.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            let _ = std::fs::remove_file(&target);
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &target)?;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn files_under(root: &Path, dir: &Path, found: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(root, &path, found);
        } else if path.is_file() {
            found.insert(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

/// The base repository in `dest`. Without history, its final files are
/// there, untracked, in a repository with no commit.
pub fn build_repo(repo: &Repo, dest: &Path, history: bool) -> Result<Git> {
    if dest.exists() {
        return error(format!("{} exists", dest.display()));
    }
    std::fs::create_dir_all(dest)?;
    let git = Git::new(dest);
    git.run(&["init", "-q", "-b", "main"])?;
    if !history {
        copy_tree(&repo.files, dest)?;
        for commit in &repo.commits {
            for op in ops_of(commit) {
                run_op(&op, dest, &git)?;
            }
        }
        return Ok(git);
    }
    let mut introduced = BTreeSet::new();
    for commit in &repo.commits {
        for path in strings(commit.get("paths")) {
            let source = repo.files.join(&path);
            if !source.is_file() {
                return error(format!("repo {}: no file {path}", repo.name));
            }
            let target = dest.join(&path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, &target)?;
            introduced.insert(path);
        }
        for op in ops_of(commit) {
            run_op(&op, dest, &git)?;
        }
        git.run(&["add", "-A"])?;
        let message = commit
            .get("message")
            .and_then(Toml::as_str)
            .unwrap_or_default();
        git.run(&["commit", "-q", "-m", message])?;
    }
    let mut present = BTreeSet::new();
    files_under(&repo.files, &repo.files, &mut present);
    let missing: Vec<&String> = present.difference(&introduced).collect();
    if !missing.is_empty() {
        let list: Vec<String> = missing.iter().map(|m| repr(m)).collect();
        return error(format!(
            "repo {}: files no commit introduces: [{}]",
            repo.name,
            list.join(", ")
        ));
    }
    Ok(git)
}

fn ops_of(table: &Table) -> Vec<Table> {
    table
        .get("ops")
        .and_then(Toml::as_array)
        .map(|a| a.iter().filter_map(|o| o.as_table().cloned()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- operations

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn inside(root: &Path, path: &str) -> Result<PathBuf> {
    let resolved = lexical(&root.canonicalize()?.join(path));
    if !resolved.starts_with(root.canonicalize()?) {
        return error(format!("path outside the repository: {path}"));
    }
    Ok(root.join(path))
}

fn field<'a>(op: &'a Table, key: &str) -> Result<&'a Toml> {
    op.get(key).ok_or_else(|| ScenarioError(repr(key)))
}

fn text_field<'a>(op: &'a Table, key: &str) -> Result<&'a str> {
    field(op, key)?
        .as_str()
        .ok_or_else(|| ScenarioError(format!("{} is not text", repr(key))))
}

/// The files an operation names: `path`, or `paths`, globs allowed.
fn files_of(root: &Path, op: &Table) -> Result<Vec<PathBuf>> {
    let patterns = match op
        .get("paths")
        .and_then(Toml::as_array)
        .filter(|p| !p.is_empty())
    {
        Some(paths) => paths
            .iter()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect(),
        None => vec![text_field(op, "path")?.to_owned()],
    };
    let mut found = Vec::new();
    for pattern in patterns {
        let mut matches: Vec<PathBuf> = glob_files(&root.join(&pattern))
            .into_iter()
            .filter(|p| p.is_file() && !p.components().any(|c| c.as_os_str() == ".git"))
            .collect();
        matches.sort();
        if matches.is_empty() {
            return error(format!("no file matches {pattern}"));
        }
        found.extend(matches);
    }
    Ok(found)
}

fn read(path: &Path) -> Result<String> {
    String::from_utf8(std::fs::read(path)?)
        .map_err(|e| ScenarioError(format!("{}: {e}", path.display())))
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text.as_bytes())?;
    Ok(())
}

/// The operation's text, or `repeat` copies of it with `{{i}}` counting
/// from 0, for large files.
fn text_of(op: &Table) -> Result<String> {
    let text = text_field(op, "text")?;
    let Some(repeat) = op.get("repeat").and_then(Toml::as_integer) else {
        return Ok(text.to_owned());
    };
    let join = op.get("join").and_then(Toml::as_str).unwrap_or("");
    Ok((0..repeat)
        .map(|i| text.replace("{{i}}", &i.to_string()))
        .collect::<Vec<_>>()
        .join(join))
}

/// Python's `re.sub` replacement template, expanded for one match.
fn expand_template(template: &str, found: &fancy_regex::Captures<'_, str>) -> Result<String> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('g') => {
                if chars.next() != Some('<') {
                    return error("missing < in \\g");
                }
                let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
                let group = match name.parse::<usize>() {
                    Ok(n) => found.get(n),
                    Err(_) => found.name(&name),
                };
                out.push_str(group.map_or("", |g| g.as_str()));
            }
            Some(d) if d.is_ascii_digit() => {
                let mut number = d.to_string();
                if let Some(&next) = chars.peek().filter(|n| n.is_ascii_digit()) {
                    number.push(next);
                    chars.next();
                }
                let n: usize = number.parse().unwrap_or(0);
                out.push_str(found.get(n).map_or("", |g| g.as_str()));
            }
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Ok(out)
}

fn op_regex(root: &Path, op: &Table) -> Result<()> {
    let pattern = text_field(op, "pattern")?;
    let regex = Regex::new(&format!("(?m){pattern}"))
        .map_err(|e| ScenarioError(format!("{pattern}: {e}")))?;
    let repl = text_field(op, "repl")?;
    for path in files_of(root, op)? {
        let text = read(&path)?;
        let (mut out, mut last, mut count) = (String::new(), 0, 0);
        for found in regex.captures_iter(text.as_str()) {
            let found = found.map_err(|e| ScenarioError(e.to_string()))?;
            let whole = found.get(0).expect("group 0");
            out.push_str(&text[last..whole.start()]);
            out.push_str(&expand_template(repl, &found)?);
            last = whole.end();
            count += 1;
        }
        out.push_str(&text[last..]);
        let optional = op.get("optional").and_then(Toml::as_bool).unwrap_or(false);
        if count == 0 && !optional {
            return error(format!(
                "{}: no match for {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                repr(pattern)
            ));
        }
        write(&path, &out)?;
    }
    Ok(())
}

fn run_git_op(root: &Path, git: &Git, op: &Table) -> Result<()> {
    let _ = root;
    let args: Vec<String> = strings(Some(field(op, "args")?));
    let mut resolved = Vec::new();
    for arg in &args {
        let mut out = String::new();
        let mut last = 0;
        for found in COMMIT_REF.captures_iter(arg.as_str()) {
            let found = found.map_err(|e| ScenarioError(e.to_string()))?;
            let whole = found.get(0).expect("group 0");
            out.push_str(&arg[last..whole.start()]);
            let subject = &found[1];
            let log = git.run(&[
                "log",
                "--all",
                "--format=%H",
                "--fixed-strings",
                "--grep",
                subject,
            ])?;
            let hashes: Vec<&str> = log.split_whitespace().collect();
            if hashes.len() != 1 {
                return error(format!("{} commits match {}", hashes.len(), repr(subject)));
            }
            out.push_str(hashes[0]);
            last = whole.end();
        }
        out.push_str(&arg[last..]);
        resolved.push(out);
    }
    let refs: Vec<&str> = resolved.iter().map(String::as_str).collect();
    git.call(
        &refs,
        op.get("check").and_then(Toml::as_bool).unwrap_or(true),
    )?;
    Ok(())
}

pub fn run_op(op: &Table, root: &Path, git: &Git) -> Result<()> {
    match text_field(op, "op")? {
        "write" => write(&inside(root, text_field(op, "path")?)?, &text_of(op)?),
        "append" => {
            for path in files_of(root, op)? {
                write(&path, &(read(&path)? + &text_of(op)?))?;
            }
            Ok(())
        }
        "prepend" => {
            for path in files_of(root, op)? {
                write(&path, &(text_of(op)? + &read(&path)?))?;
            }
            Ok(())
        }
        "replace" => {
            // Literal replacement; `old` must occur exactly once unless `all`.
            let (old, new) = (text_field(op, "old")?, text_field(op, "new")?);
            for path in files_of(root, op)? {
                let text = read(&path)?;
                let count = if old.is_empty() {
                    text.chars().count() + 1
                } else {
                    text.matches(old).count()
                };
                let all = op.get("all").and_then(Toml::as_bool).unwrap_or(false);
                if count == 0 || (count > 1 && !all) {
                    return error(format!(
                        "{}: {count} occurrences of {}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        repr(old)
                    ));
                }
                write(&path, &text.replace(old, new))?;
            }
            Ok(())
        }
        "regex" => op_regex(root, op),
        "remove" => {
            let path = inside(root, text_field(op, "path")?)?;
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
            Ok(())
        }
        "move" => {
            let target = inside(root, text_field(op, "to")?)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(inside(root, text_field(op, "from")?)?, target)?;
            Ok(())
        }
        "chmod" => {
            use std::os::unix::fs::PermissionsExt;
            let mode = u32::from_str_radix(text_field(op, "mode")?, 8)
                .map_err(|e| ScenarioError(e.to_string()))?;
            let path = inside(root, text_field(op, "path")?)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
            Ok(())
        }
        "symlink" => {
            let path = inside(root, text_field(op, "path")?)?;
            std::os::unix::fs::symlink(text_field(op, "target")?, path)?;
            Ok(())
        }
        "binary" => {
            let seed = op.get("seed").and_then(Toml::as_integer).unwrap_or(0);
            let size = field(op, "size")?
                .as_integer()
                .ok_or_else(|| ScenarioError("'size' is not a number".into()))?;
            let data = Mt::new(seed as u64).randbytes(size as usize);
            let path = inside(root, text_field(op, "path")?)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, data)?;
            Ok(())
        }
        "git" => run_git_op(root, git, op),
        other => error(format!("unknown op {}", repr(other))),
    }
}

/// Builds `case`'s repository in `dest`, with its changes staged.
pub fn apply(case: &Case, repo: &Repo, dest: &Path) -> Result<Git> {
    let git = build_repo(repo, dest, case.history)?;
    for (n, op) in case.ops.iter().enumerate() {
        run_op(op, dest, &git).map_err(|e| {
            let name = op.get("op").and_then(Toml::as_str).unwrap_or("?");
            ScenarioError(format!("case {}, op {} ({name}): {e}", case.id, n + 1))
        })?;
    }
    if case.stage == "all" {
        git.run(&["add", "-A"])?;
    }
    Ok(git)
}

// ---------------------------------------------------------------- capture

/// The trace inputs smart-commit records for `workdir`'s staged changes,
/// from its mock backend, with an empty configuration; or the error when it
/// refuses to generate.
pub fn capture(workdir: &Path, binary: &str) -> std::result::Result<Value, String> {
    let home = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let trace = home.path().join("trace.json");
    let h = home.path().to_string_lossy().into_owned();
    let mut env = git_env(workdir, 0);
    for (key, value) in [
        ("HOME", h.clone()),
        ("XDG_CONFIG_HOME", format!("{h}/config")),
        ("XDG_CACHE_HOME", format!("{h}/cache")),
        ("XDG_DATA_HOME", format!("{h}/data")),
    ] {
        env.retain(|(k, _)| k != key);
        env.push((key.to_owned(), value));
    }
    let output = Command::new(binary)
        .arg("-C")
        .arg(workdir)
        .args(["--backend", "mock", "--mock-response", "mock", "--trace"])
        .arg(&trace)
        .env_clear()
        .envs(env)
        .output()
        .map_err(|e| format!("cannot run {binary}: {e}"))?;
    if !output.status.success() || !trace.exists() {
        let text = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        return Err(String::from_utf8_lossy(text).trim().to_owned());
    }
    let value: Value =
        serde_json::from_str(&std::fs::read_to_string(&trace).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    value
        .get("inputs")
        .cloned()
        .ok_or_else(|| "no inputs in the trace".to_owned())
}

/// The `meta.jsonl` line: what `score` and `make-examples` read, plus
/// what the scenarios add (level, tags, split, expected words).
pub fn meta_of(case: &Case, repo: &Repo, inputs: &Value) -> Value {
    let convention = if repo.convention == "conventional" && case.history {
        "conventional"
    } else {
        "free"
    };
    // git's own messages (Merge ..., Revert "...") count as types merge and revert.
    let kind = match CONVENTIONAL.captures(case.target.as_str()).ok().flatten() {
        Some(m) => Some(m[1].to_owned()),
        None => Some(plain(case.target.split(' ').next().unwrap_or(""))).filter(|k| !k.is_empty()),
    };
    let history = inputs["subjects"].as_array().map_or(0, Vec::len);
    let mut meta = Map::new();
    meta.insert("repo".into(), json!(format!("scenario/{}", case.repo)));
    meta.insert("hash".into(), json!(case.id));
    meta.insert("target".into(), json!(case.target));
    meta.insert("type".into(), json!(kind));
    meta.insert("history".into(), json!(history));
    meta.insert("set".into(), json!(convention));
    meta.insert("title".into(), json!(case.title));
    meta.insert("level".into(), json!(case.level));
    meta.insert("split".into(), json!(case.split));
    meta.insert("tags".into(), json!(case.tags));
    meta.insert("expect".into(), json!(case.expect));
    Value::Object(meta)
}

/// Lowercase without accents, which `expect` patterns are matched on.
pub fn plain(text: &str) -> String {
    text.nfkd()
        .filter(|c| canonical_combining_class(*c) == 0)
        .collect::<String>()
        .to_lowercase()
}

/// Whether `subject` has what a right message must say: every pattern
/// matches. None when the case lists none.
pub fn expected(subject: &str, patterns: &[String]) -> Option<bool> {
    if patterns.is_empty() {
        return None;
    }
    let text = plain(subject);
    Some(patterns.iter().all(|p| {
        Regex::new(p)
            .ok()
            .and_then(|r| r.is_match(&text).ok())
            .unwrap_or(false)
    }))
}

pub fn select<'a>(cases: &'a [Case], split: &str, only: &str) -> Vec<&'a Case> {
    cases
        .iter()
        .filter(|c| (split == "all" || c.split == split) && c.id.starts_with(only))
        .collect()
}

#[cfg(test)]
#[path = "scenarios_tests.rs"]
mod tests;
