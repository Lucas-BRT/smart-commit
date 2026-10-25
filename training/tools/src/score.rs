//! Scores `smart-commit eval` traces against the real subjects.
//!
//! Per set (conventional, free) and overall:
//! - first_try: the first reply passed the sanitizer
//! - fallback: every attempt failed; the message came from the file list
//! - copied: the subject is one of the history subjects verbatim
//! - first_copied: the first reply repeated a history subject (smart-commit
//!   rejects that and retries, so copied alone hides it)
//! - type_match: same Conventional type as the real subject (conventional
//!   set), with spellings of one type folded (`tests` is `test`)
//! - type_macro_f1: the F1 of each real type, averaged over the types, with
//!   each type's precision, recall and F1 in type_scores: type_match is
//!   dominated by `fix`, the most frequent type
//! - scope_known: no scope, or one already used in the history (conventional
//!   set)
//! - scope_fits: the scope was used by history commits touching a directory
//!   the staged files are in, or there is no scope and no history scope does
//!   (conventional set; needs `history_files` in the inputs). scope_fits_real
//!   is the same for the real subject
//! - word_f1: word overlap with the real subject, a cheap proxy for content
//! - exact: identical to the real subject
//! - unsupported: names a specific (a version or number, an identifier in
//!   camelCase, snake_case or backticks) found neither in the diff, the paths
//!   nor the history: likely invented
//! - ungrounded: no content word of the description is in the diff or the
//!   paths: likely vague ("fix typo", "update code")
//! - unsupported_real / ungrounded_real: the same for the real subject, the
//!   level a human reaches with the same diff
//! - expected: the subject matches every `expect` pattern of the case (only
//!   for sets that list them, as the scenarios do)
//!
//! Sets whose meta has a `level` (the scenarios) are also reported per level.
//!
//! With `--against OTHER.jsonl` (traces of the same cases, e.g. the same model
//! with another prompt), it compares the two case by case instead: for each
//! yes/no metric, how many cases each version gets right that the other does
//! not, and the exact p of McNemar's test on that split.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::build_inputs::{conventional_type, fold_type};
use crate::scenarios::expected;

static SCOPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]+\(([^()]+)\)").expect("a valid pattern"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").expect("a valid pattern"));
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[a-z]+(?:\([^()]*\))?!?:\s*|\W+\s*)").expect("a valid pattern")
});
static SPECIFIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"`([^`]+)`",                                    // `anything`
        r"|\b(v?\d+(?:\.\d+)+[\w.-]*)",                  // versions: 1.2, v5.5.0-beta.6
        r"|\b(\d{2,})\b",                                // numbers of 2+ digits
        r"|\b([a-z]+[A-Z]\w*|[A-Z][a-z0-9]+[A-Z]\w*)\b", // camelCase, PascalCase
        r"|\b(\w+_\w+)\b",                               // snake_case
    ))
    .expect("a valid pattern")
});

const STOP: &str = "a an the and or of to in on for with from by into onto when then
than that this these those it its is are be was were as at not no only all
any some more less new old use used using add adds added fix fixes fixed
update updates updated remove removes removed make makes made change changes
changed support allow allows move moves rename renames improve improves
refactor clean cleanup minor small bug bugs issue issues code file files
test tests docs doc";

/// The subject without its Conventional header or leading emoji.
pub fn description(subject: &str) -> String {
    HEADER.replace(subject, "").into_owned()
}

pub fn specifics(text: &str) -> Vec<String> {
    SPECIFIC
        .captures_iter(text)
        .filter_map(|m| {
            (1..=5)
                .filter_map(|n| m.get(n))
                .map(|g| g.as_str())
                .find(|g| !g.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn strings<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value[key]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

pub fn evidence(inputs: &Value) -> String {
    let mut parts: Vec<&str> = strings(inputs, "subjects");
    for f in inputs["files"].as_array().into_iter().flatten() {
        parts.push(f["path"].as_str().unwrap_or(""));
        parts.push(f["old_path"].as_str().unwrap_or(""));
        parts.push(f["hunks"].as_str().unwrap_or(""));
    }
    parts.join("\n").to_lowercase()
}

/// The directory a file is in, with a trailing slash; "" at the root.
pub fn area(path: &str) -> String {
    path.rsplit_once('/')
        .map_or(String::new(), |(dir, _)| format!("{dir}/"))
}

pub fn scope_fits(subject: &str, inputs: &Value) -> Option<bool> {
    let history_files = inputs.get("history_files")?.as_array()?;
    let staged: BTreeSet<String> = inputs["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| area(f["path"].as_str().unwrap_or("")))
        .collect();
    let mut covered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (previous, files) in strings(inputs, "subjects").into_iter().zip(history_files) {
        if let Some(m) = SCOPE.captures(previous) {
            covered.entry(m[1].to_owned()).or_default().extend(
                files
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| area(p.as_str().unwrap_or(""))),
            );
        }
    }
    Some(match SCOPE.captures(subject) {
        None => !covered
            .values()
            .any(|areas| areas.intersection(&staged).next().is_some()),
        Some(m) => covered
            .get(&m[1])
            .is_some_and(|areas| areas.intersection(&staged).next().is_some()),
    })
}

pub fn unsupported(subject: &str, known: &str) -> bool {
    specifics(&description(subject))
        .iter()
        .any(|s| !known.contains(&s.to_lowercase()))
}

fn ungrounded(subject: &str, diff: &str) -> bool {
    let stop: BTreeSet<&str> = STOP.split_whitespace().collect();
    let content: Vec<String> = words(&description(subject))
        .into_iter()
        .filter(|w| w.chars().count() >= 3 && !stop.contains(w.as_str()))
        .collect();
    !content.iter().any(|w| diff.contains(w.as_str()))
}

fn words(text: &str) -> Vec<String> {
    WORD.find_iter(&text.to_lowercase())
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn counter(words: Vec<String>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for w in words {
        *counts.entry(w).or_insert(0) += 1;
    }
    counts
}

fn f1(a: &str, b: &str) -> f64 {
    let (x, y) = (counter(words(a)), counter(words(b)));
    let common: usize = x
        .iter()
        .map(|(w, n)| (*n).min(*y.get(w).unwrap_or(&0)))
        .sum();
    if common == 0 {
        return 0.0;
    }
    let p = common as f64 / x.values().sum::<usize>() as f64;
    let r = common as f64 / y.values().sum::<usize>() as f64;
    2.0 * p * r / (p + r)
}

/// Python's `round(x, 3)`: correctly rounded, ties to even.
pub fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap_or(x)
}

/// A count of (real, generated) type pairs, in the order first seen.
#[derive(Default, Debug, Clone)]
pub struct Pairs(pub Vec<((String, Option<String>), usize)>);

impl Pairs {
    pub fn add(&mut self, real: &str, generated: Option<String>) {
        let key = (real.to_owned(), generated);
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => self.0.push((key, 1)),
        }
    }

    /// Like `Counter.most_common()`: by count, ties in the order first seen.
    pub fn most_common(&self) -> Vec<&((String, Option<String>), usize)> {
        let mut all: Vec<_> = self.0.iter().collect();
        all.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        all
    }
}

/// Precision, recall and F1 of each real type, from (real, generated)
/// counts, and their mean over the real types (macro F1): accuracy alone is
/// dominated by `fix`, the most frequent type.
pub fn per_type(pairs: &Pairs) -> (Value, f64) {
    let mut real: BTreeMap<&str, usize> = BTreeMap::new();
    let mut generated: BTreeMap<Option<&str>, usize> = BTreeMap::new();
    let mut hits: BTreeMap<&str, usize> = BTreeMap::new();
    for ((r, g), n) in &pairs.0 {
        *real.entry(r).or_insert(0) += n;
        *generated.entry(g.as_deref()).or_insert(0) += n;
        if g.as_deref() == Some(r.as_str()) {
            *hits.entry(r).or_insert(0) += n;
        }
    }
    let mut order: Vec<&str> = real.keys().copied().collect();
    order.sort_by(|a, b| real[b].cmp(&real[a]).then(a.cmp(b)));
    let mut scores = Map::new();
    let mut f1s = Vec::new();
    for t in order {
        let hit = *hits.get(t).unwrap_or(&0) as f64;
        let made = *generated.get(&Some(t)).unwrap_or(&0) as f64;
        let p = if made > 0.0 { hit / made } else { 0.0 };
        let r = hit / real[t] as f64;
        let f = if p + r > 0.0 {
            2.0 * p * r / (p + r)
        } else {
            0.0
        };
        f1s.push(round3(f));
        scores.insert(
            t.to_owned(),
            json!({"n": real[t], "precision": round3(p), "recall": round3(r), "f1": round3(f)}),
        );
    }
    let macro_f1 = if f1s.is_empty() {
        0.0
    } else {
        f1s.iter().sum::<f64>() / f1s.len() as f64
    };
    (Value::Object(scores), round3(macro_f1))
}

/// The report sections a case counts in.
pub fn groups(m: &Value) -> Vec<String> {
    let mut keys = vec![m["set"].as_str().unwrap_or("").to_owned(), "all".to_owned()];
    if let Some(level) = m.get("level") {
        keys.push(format!("level {level}"));
    }
    keys
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Metric {
    Yes(bool),
    Number(f64),
}

impl Metric {
    fn value(self) -> f64 {
        match self {
            Self::Yes(b) => f64::from(u8::from(b)),
            Self::Number(n) => n,
        }
    }
}

/// The metrics of one case, in a fixed order: `m` its meta line, `t` its
/// trace.
pub fn row_of(m: &Value, t: &Value) -> Vec<(&'static str, Metric)> {
    let g = &t["generation"];
    let subject = g["message"]["subject"].as_str().unwrap_or("");
    let target = m["target"].as_str().unwrap_or("");
    let history = strings(&t["inputs"], "subjects");
    let known = evidence(&t["inputs"]);
    let mut without = t["inputs"].clone();
    without["subjects"] = json!([]);
    let diff = evidence(&without);
    // Messages from rules (lockfiles, generated files) have no attempt.
    let first = g["attempts"]
        .get(0)
        .map(|a| &a["rejected"])
        .filter(|r| !r.is_null());
    let first_copied = match first {
        Some(Value::Object(map)) => map.contains_key("copied_history"),
        Some(Value::String(text)) => text.contains("copied_history"),
        _ => false,
    };
    use Metric::{Number, Yes};
    let mut row = vec![
        ("unsupported", Yes(unsupported(subject, &known))),
        ("unsupported_real", Yes(unsupported(target, &known))),
        ("ungrounded", Yes(ungrounded(subject, &diff))),
        ("ungrounded_real", Yes(ungrounded(target, &diff))),
        ("first_try", Yes(first.is_none())),
        ("fallback", Yes(g["source"] == "fallback")),
        ("copied", Yes(history.contains(&subject))),
        ("first_copied", Yes(first_copied)),
        ("word_f1", Number(f1(subject, target))),
        ("exact", Yes(subject == target)),
    ];
    let patterns: Vec<String> = strings(m, "expect")
        .into_iter()
        .map(str::to_owned)
        .collect();
    if !patterns.is_empty() {
        row.push((
            "expected",
            Yes(expected(subject, &patterns).unwrap_or(false)),
        ));
    }
    if m["set"] == "conventional" {
        let real_type = fold_type(m["type"].as_str().unwrap_or("")).to_owned();
        row.push((
            "type_match",
            Yes(conventional_type(subject).as_deref() == Some(real_type.as_str())),
        ));
        let scopes: BTreeSet<&str> = history
            .iter()
            .filter_map(|h| SCOPE.captures(h).map(|s| s.get(1).expect("group").as_str()))
            .collect();
        let scope_known = SCOPE
            .captures(subject)
            .is_none_or(|s| scopes.contains(&s[1]));
        row.push(("scope_known", Yes(scope_known)));
        if let Some(fits) = scope_fits(subject, &t["inputs"]) {
            row.push(("scope_fits", Yes(fits)));
            row.push((
                "scope_fits_real",
                Yes(scope_fits(target, &t["inputs"]).unwrap_or(false)),
            ));
        }
    }
    row
}

/// Exact two-sided p of McNemar's test: the chance of a split at least this
/// uneven between cases that got better and worse, were neither version
/// better.
pub fn mcnemar(gained: usize, lost: usize) -> f64 {
    let n = gained + lost;
    if n == 0 {
        return 1.0;
    }
    // sum of C(n, i) / 2^n for i <= min(gained, lost), term by term.
    let mut term = 0.5f64.powi(n as i32);
    let mut tail = 0.0;
    for i in 0..=gained.min(lost) {
        tail += term;
        term = term * (n - i) as f64 / (i + 1) as f64;
    }
    (2.0 * tail).min(1.0)
}

/// Per section and yes/no metric, the cases `other` turns from false to true
/// (gained) and from true to false (lost) against `base`, with McNemar's p.
/// Averages hide that both versions answer the same cases. For the metrics
/// where true is bad (fallback, copied, first_copied, unsupported,
/// ungrounded), gained means worse.
pub fn paired(meta: &[Value], base: &[Value], other: &[Value]) -> Value {
    let mut counts: Vec<(String, BTreeMap<&'static str, [usize; 2]>)> = Vec::new();
    for ((m, a), b) in meta.iter().zip(base).zip(other) {
        let before = row_of(m, a);
        for (k, v) in row_of(m, b) {
            let Metric::Yes(v) = v else { continue };
            let Some(Metric::Yes(was)) = before.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
            else {
                continue;
            };
            if k.ends_with("_real") {
                continue;
            }
            for key in groups(m) {
                let at = match counts.iter().position(|(name, _)| *name == key) {
                    Some(at) => at,
                    None => {
                        counts.push((key, BTreeMap::new()));
                        counts.len() - 1
                    }
                };
                let pair = counts[at].1.entry(k).or_insert([0, 0]);
                pair[0] += usize::from(v && !was);
                pair[1] += usize::from(was && !v);
            }
        }
    }
    let mut report = Map::new();
    for (key, metrics) in counts {
        let mut section = Map::new();
        for (k, [gained, lost]) in metrics {
            section.insert(
                k.to_owned(),
                json!({"gained": gained, "lost": lost, "p": round3(mcnemar(gained, lost))}),
            );
        }
        report.insert(key, Value::Object(section));
    }
    Value::Object(report)
}

/// The report of one run: per section, the means of the metrics.
pub fn report(meta: &[Value], traces: &[Value]) -> Value {
    #[derive(Default)]
    struct Section {
        n: usize,
        sums: BTreeMap<&'static str, (f64, usize)>,
        types: Pairs,
    }
    let mut sections: Vec<(String, Section)> = Vec::new();
    for (m, t) in meta.iter().zip(traces) {
        let row = row_of(m, t);
        for key in groups(m) {
            let at = match sections.iter().position(|(name, _)| *name == key) {
                Some(at) => at,
                None => {
                    sections.push((key, Section::default()));
                    sections.len() - 1
                }
            };
            let section = &mut sections[at].1;
            section.n += 1;
            for (k, v) in &row {
                let entry = section.sums.entry(k).or_insert((0.0, 0));
                entry.0 += v.value();
                entry.1 += 1;
            }
            if m["set"] == "conventional" {
                let real_type = fold_type(m["type"].as_str().unwrap_or("")).to_owned();
                let subject = t["generation"]["message"]["subject"].as_str().unwrap_or("");
                section.types.add(&real_type, conventional_type(subject));
            }
        }
    }
    let mut report = Map::new();
    for (key, section) in sections {
        let mut out = Map::new();
        out.insert("n".into(), json!(section.n));
        for (k, (sum, count)) in &section.sums {
            out.insert((*k).to_owned(), json!(round3(sum / *count as f64)));
        }
        if !section.types.0.is_empty() {
            let (scores, macro_f1) = per_type(&section.types);
            out.insert("type_macro_f1".into(), json!(macro_f1));
            let crossed: Map<String, Value> = section
                .types
                .most_common()
                .into_iter()
                .map(|((r, g), n)| (format!("{r}->{}", g.as_deref().unwrap_or("None")), json!(n)))
                .collect();
            out.insert("types (real->generated)".into(), Value::Object(crossed));
            out.insert("type_scores".into(), scores);
        }
        report.insert(key, Value::Object(out));
    }
    Value::Object(report)
}

/// Python's `repr` of a JSON value, which the text report prints.
pub fn pyrepr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) if n.is_f64() => crate::pyjson::float(n.as_f64().unwrap_or(0.0)),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            let quote = if s.contains('\'') && !s.contains('"') {
                '"'
            } else {
                '\''
            };
            let escaped = s.replace('\\', "\\\\").replace('\n', "\\n");
            let escaped = if quote == '\'' {
                escaped.replace('\'', "\\'")
            } else {
                escaped
            };
            format!("{quote}{escaped}{quote}")
        }
        Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(pyrepr).collect::<Vec<_>>().join(", ")
            )
        }
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", pyrepr(&Value::String(k.clone())), pyrepr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod tests;
