//! Models on evaluation sets, against a baseline: `smart-commit eval` for
//! each model once (the traces stay in `OUT/SET/NAME.jsonl`, and a model
//! already evaluated is not run again), then the metrics of `score` for each,
//! and, for each metric, how many cases a model gains and loses against the
//! baseline (paired, with McNemar's p).
//!
//! Beside the metrics, the habits: the share of replies with a scope, the
//! count of each type, next to the real commits', and the share of content
//! words found in none of the history, the paths and the diff (`novel`). A
//! model that stops writing scopes has a `scope_known` of 1.0, and one that
//! writes `chore` for everything a `first_try` of 1.0: only the habits show
//! it. `read` also prints some cases side by side, the real subject and each
//! model's, which is how a new habit is seen that no metric counts.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::build_inputs::conventional_type;
use crate::jsonl;
use crate::score::{self, SCOPE, evidence, novel_words};

/// Metrics where a case flagged is a case right, and where it is a case wrong.
const GOOD: [&str; 6] = [
    "first_try",
    "type_match",
    "scope_known",
    "scope_fits",
    "exact",
    "expected",
];
const BAD: [&str; 5] = [
    "ungrounded",
    "unsupported",
    "fallback",
    "first_copied",
    "copied",
];
const SHOWN: [&str; 13] = [
    "type_match",
    "type_macro_f1",
    "scope_known",
    "scoped",
    "novel",
    "ungrounded",
    "first_copied",
    "first_try",
    "fallback",
    "word_f1",
    "exact",
    "expected",
    "chars",
];
const GROUPS: [&str; 2] = ["conventional", "free"];

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub gguf: PathBuf,
}

impl Model {
    /// `NAME=GGUF`.
    pub fn parse(text: &str) -> Result<Self> {
        let (name, gguf) = text
            .split_once('=')
            .filter(|(name, gguf)| !name.is_empty() && !gguf.is_empty())
            .with_context(|| format!("{text:?} is not NAME=GGUF"))?;
        Ok(Self {
            name: name.to_owned(),
            gguf: gguf.into(),
        })
    }
}

pub struct Options {
    pub out: PathBuf,
    /// `smart-commit`.
    pub binary: PathBuf,
    pub sets: Vec<PathBuf>,
    pub baseline: Model,
    pub candidates: Vec<Model>,
    pub rules: String,
    pub diff: String,
    pub device: String,
    /// How many cases to print side by side, half of each set; 0 for none.
    pub read: usize,
}

/// Cases a model improves over the baseline, net: a flag gained is better for
/// the `GOOD` metrics and worse for the `BAD` ones.
pub fn better(metric: &str, gained: i64, lost: i64) -> i64 {
    if GOOD.contains(&metric) {
        gained - lost
    } else if BAD.contains(&metric) {
        lost - gained
    } else {
        0
    }
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

/// Writes the traces of `gguf` on `inputs` to `out` unless they are all there;
/// whether it ran. `smart-commit` takes an absolute model path only, and this
/// may run from anywhere; a failed run leaves no file and says what it said.
pub fn run_eval(
    binary: &Path,
    gguf: &Path,
    inputs: &Path,
    out: &Path,
    options: &Options,
) -> Result<bool> {
    let wanted = line_count(inputs);
    if wanted > 0 && line_count(out) == wanted {
        return Ok(false);
    }
    std::fs::create_dir_all(out.parent().context("an output without a directory")?)?;
    let gguf =
        std::fs::canonicalize(gguf).with_context(|| format!("no model at {}", gguf.display()))?;
    let config_home = tempfile::TempDir::new()?;
    let done = Command::new(binary)
        .arg("eval")
        .arg(inputs)
        .args([
            "--device",
            &options.device,
            "--rules",
            &options.rules,
            "--diff",
            &options.diff,
        ])
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("SMART_COMMIT_GGUF", &gguf)
        .output()
        .with_context(|| format!("cannot run {}", binary.display()))?;
    if !done.status.success() {
        let stderr = String::from_utf8_lossy(&done.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(1500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        bail!(
            "{} eval of {} failed ({}):\n{tail}",
            binary.display(),
            gguf.display(),
            done.status
        );
    }
    std::fs::write(out, done.stdout)?;
    Ok(true)
}

fn subject(trace: &Value) -> &str {
    trace["generation"]["message"]["subject"]
        .as_str()
        .unwrap_or("")
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The mean length of the subjects, in characters.
pub fn mean_chars(subjects: &[&str]) -> f64 {
    if subjects.is_empty() {
        return 0.0;
    }
    round1(subjects.iter().map(|s| s.chars().count()).sum::<usize>() as f64 / subjects.len() as f64)
}

/// Per set, the share of subjects with a scope, the count of each type (`-`
/// for none), most common first, and the share of content words found in
/// nothing the model was given (`known`: the evidence of each case).
pub fn habits(meta: &[Value], subjects: &[&str], known: &[String]) -> Value {
    let mut found = Map::new();
    for group in GROUPS {
        let (mine, evidence): (Vec<&str>, Vec<&String>) = meta
            .iter()
            .zip(subjects)
            .zip(known)
            .filter(|((m, _), _)| m["set"] == group)
            .map(|((_, s), k)| (*s, k))
            .unzip();
        let (words, novel) = mine
            .iter()
            .zip(&evidence)
            .map(|(s, k)| novel_words(s, k))
            .fold((0, 0), |(w, n), (a, b)| (w + a, n + b));
        let mut counts: Vec<(String, usize)> = Vec::new();
        for s in &mine {
            let kind = conventional_type(s).unwrap_or_else(|| "-".to_owned());
            match counts.iter_mut().find(|(k, _)| *k == kind) {
                Some((_, n)) => *n += 1,
                None => counts.push((kind, 1)),
            }
        }
        counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let scoped =
            mine.iter().filter(|s| SCOPE.is_match(s)).count() as f64 / mine.len().max(1) as f64;
        found.insert(
            group.to_owned(),
            json!({
                "n": mine.len(),
                "scoped": (scoped * 1000.0).round() / 1000.0,
                "novel": (novel as f64 / words.max(1) as f64 * 1000.0).round() / 1000.0,
                "types": counts.into_iter().map(|(k, n)| (k, json!(n))).collect::<Map<_, _>>(),
            }),
        );
    }
    Value::Object(found)
}

/// Columns of `SHOWN` that at least one model has.
fn columns(rows: &[Map<String, Value>]) -> Vec<&'static str> {
    SHOWN
        .into_iter()
        .filter(|m| rows.iter().any(|r| r.contains_key(*m)))
        .collect()
}

fn table(rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..rows[0].len())
        .map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, cell)| {
                    let pad = " ".repeat(widths[i] - cell.chars().count());
                    if i == 0 {
                        format!("{cell}{pad}")
                    } else {
                        format!("{pad}{cell}")
                    }
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The metrics a model shows for a group: its score, `scoped` and `chars`.
fn shown(entry: &Value, group: &str) -> Map<String, Value> {
    let mut scores = entry["scores"][group]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if group == "conventional" {
        scores.insert("scoped".into(), entry["habits"][group]["scoped"].clone());
    }
    scores.insert("novel".into(), entry["habits"][group]["novel"].clone());
    scores.insert("chars".into(), entry["mean_subject_chars"].clone());
    scores
}

fn cell(scores: &Map<String, Value>, metric: &str) -> String {
    match (metric, scores.get(metric).and_then(Value::as_f64)) {
        ("chars", Some(x)) => format!("{x:.1}"),
        (_, Some(x)) => format!("{x:.3}"),
        _ => "-".to_owned(),
    }
}

/// The net gains against the baseline of a model, `metric+n`, with `*` where
/// p is under 0.05.
fn net(entry: &Value, group: &str) -> String {
    let nets: Vec<String> = entry["against_baseline"][group]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(metric, _)| SHOWN.contains(&metric.as_str()))
        .filter_map(|(metric, p)| {
            let n = better(metric, p["gained"].as_i64()?, p["lost"].as_i64()?);
            let star = if p["p"].as_f64()? < 0.05 { "*" } else { "" };
            (n != 0).then(|| format!("{metric}{n:+}{star}"))
        })
        .collect();
    if nets.is_empty() {
        "-".to_owned()
    } else {
        nets.join(" ")
    }
}

/// `n` cases, half Conventional and half free, taken in the order of a hash of
/// the commit, so the same cases whatever the models.
pub fn sample(meta: &[Value], n: usize) -> Vec<usize> {
    let mut chosen = Vec::new();
    for (group, want) in [("conventional", n.div_ceil(2)), ("free", n / 2)] {
        let mut mine: Vec<(Vec<u8>, usize)> = meta
            .iter()
            .enumerate()
            .filter(|(_, m)| m["set"] == group)
            .map(|(i, m)| {
                let hash = m["hash"].as_str().unwrap_or("");
                (
                    Sha256::digest(format!("read:{hash}").as_bytes()).to_vec(),
                    i,
                )
            })
            .collect();
        mine.sort();
        chosen.extend(mine.into_iter().take(want).map(|(_, i)| i));
    }
    chosen
}

/// The cases of `sample` printed side by side: the files, the real subject and
/// each model's.
fn read(meta: &[Value], names: &[&str], traces: &[Vec<Value>], n: usize) -> String {
    let mut text = String::new();
    for i in sample(meta, n) {
        let files: Vec<&str> = traces[0][i]["inputs"]["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f["path"].as_str())
            .take(3)
            .collect();
        let group = meta[i]["set"].as_str().unwrap_or("");
        let real = meta[i]["target"].as_str().unwrap_or("");
        text.push_str(&format!(
            "\n[{group} {i}] {}\n  {:12} {real}",
            files.join(", "),
            "real"
        ));
        for (name, found) in names.iter().zip(traces) {
            text.push_str(&format!("\n  {name:12} {}", subject(&found[i])));
        }
    }
    text
}

/// Runs what is not run, scores, and returns what to print and the summary.
pub fn compare(options: &Options) -> Result<(String, Value)> {
    let models: Vec<&Model> = std::iter::once(&options.baseline)
        .chain(&options.candidates)
        .collect();
    let (mut text, mut summary) = (String::new(), Map::new());
    for set in &options.sets {
        let set_name = set
            .file_name()
            .and_then(|n| n.to_str())
            .context("a set without a name")?;
        let meta = jsonl(&set.join("meta.jsonl"))?;
        let mut traces = Vec::new();
        for model in &models {
            let path = options
                .out
                .join(set_name)
                .join(format!("{}.jsonl", model.name));
            if run_eval(
                &options.binary,
                &model.gguf,
                &set.join("inputs.jsonl"),
                &path,
                options,
            )? {
                eprintln!("{set_name}: {} evaluated", model.name);
            }
            let found = jsonl(&path)?;
            ensure!(
                found.len() == meta.len(),
                "{}: {} traces for {} cases",
                path.display(),
                found.len(),
                meta.len()
            );
            traces.push(found);
        }
        let known: Vec<String> = traces[0].iter().map(|t| evidence(&t["inputs"])).collect();
        let mut entries = Map::new();
        for (model, found) in models.iter().zip(&traces) {
            let subjects: Vec<&str> = found.iter().map(subject).collect();
            let against = if model.name == options.baseline.name {
                json!({})
            } else {
                score::paired(&meta, &traces[0], found)
            };
            entries.insert(
                model.name.clone(),
                json!({
                    "scores": score::report(&meta, found),
                    "against_baseline": against,
                    "mean_subject_chars": mean_chars(&subjects),
                    "habits": habits(&meta, &subjects, &known),
                }),
            );
        }
        let real: Vec<&str> = meta
            .iter()
            .map(|m| m["target"].as_str().unwrap_or(""))
            .collect();
        entries.insert(
            "real".into(),
            json!({"habits": habits(&meta, &real, &known)}),
        );
        for group in GROUPS {
            let rows: Vec<Map<String, Value>> = models
                .iter()
                .map(|m| shown(&entries[&m.name], group))
                .collect();
            if entries[&models[0].name]["scores"][group].is_null() {
                continue;
            }
            let columns = columns(&rows);
            let mut lines = vec![
                [
                    vec!["model".to_owned()],
                    columns.iter().map(|c| (*c).to_owned()).collect(),
                    vec!["net vs baseline (* p < 0.05)".to_owned()],
                ]
                .concat(),
            ];
            for (model, scores) in models.iter().zip(&rows) {
                lines.push(
                    [
                        vec![model.name.clone()],
                        columns.iter().map(|c| cell(scores, c)).collect(),
                        vec![net(&entries[&model.name], group)],
                    ]
                    .concat(),
                );
            }
            text.push_str(&format!("\n== {set_name} / {group}\n{}\n", table(&lines)));
            if group == "conventional" {
                let real = &entries["real"]["habits"][group];
                text.push_str(&format!(
                    "\ntypes (n={}, scoped {}, novel {})",
                    real["n"], real["scoped"], real["novel"]
                ));
                let named = std::iter::once(("real", real)).chain(
                    models
                        .iter()
                        .map(|m| (m.name.as_str(), &entries[&m.name]["habits"][group])),
                );
                for (name, h) in named {
                    let types: Vec<String> = h["types"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .take(7)
                        .map(|(k, n)| format!("{k} {n}"))
                        .collect();
                    text.push_str(&format!("\n  {name:12} {}", types.join(" ")));
                }
                text.push('\n');
            }
        }
        if options.read > 0 {
            let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
            text.push_str(&format!(
                "\n== {set_name} / {} cases side by side{}\n",
                options.read.min(meta.len()),
                read(&meta, &names, &traces, options.read)
            ));
        }
        summary.insert(set_name.to_owned(), Value::Object(entries));
    }
    std::fs::create_dir_all(&options.out)?;
    let summary = Value::Object(summary);
    std::fs::write(
        options.out.join("summary.json"),
        serde_json::to_string_pretty(&summary)? + "\n",
    )?;
    Ok((text, summary))
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
