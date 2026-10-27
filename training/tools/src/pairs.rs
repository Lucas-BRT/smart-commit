//! Preference pairs for DPO: a reply the model sampled that a check
//! rejects, against one that passes.
//!
//! `mine_pairs.py` samples several replies for each input. Each is judged by
//! the checks of `score`: a Conventional header (in the Conventional set), a
//! subject of at most 72 columns, a history subject copied, a specific (a
//! version, an identifier) found nowhere in the diff, and a description
//! sharing no word with it; and, in the Conventional set, a scope the history
//! never used (the prompt asks to omit it then) and a type other than the
//! real commit's. The rejected reply is the one that fails the most serious
//! check, in the order of [`Failure`]. The preferred one passes them all: a
//! sample of the model closest in words to the real subject, or the real
//! subject itself, whichever [`Prefer`] puts first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};

use crate::build_inputs::{conventional_type, fold_type};
use crate::score::{SCOPE, description, evidence, f1, ungrounded, unsupported};

/// The longest subject, in columns, as in smart-commit.
const SUBJECT_MAX: usize = 72;

/// A history subject is repeated nearly when every word of the description,
/// four or more, is in one of theirs and covers this share of it.
const NEAR_COPY_SHARE: f64 = 0.6;

/// Why a reply is rejected, from the most serious check to the least.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Failure {
    Empty,
    Format,
    Long,
    Copied,
    Unsupported,
    Ungrounded,
    /// A scope no history subject used (Conventional set).
    Scope,
    /// Not the type of the real commit (Conventional set).
    Type,
}

/// Which subject is preferred when the real one and samples both pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Prefer {
    /// The model's own sample closest to the real subject: what it can
    /// already write, so the pair teaches a choice and not a style.
    Sample,
    /// The real subject, when it passes.
    Real,
}

impl Failure {
    pub fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Format => "format",
            Self::Long => "long",
            Self::Copied => "copied",
            Self::Unsupported => "unsupported",
            Self::Ungrounded => "ungrounded",
            Self::Scope => "scope",
            Self::Type => "type",
        }
    }
}

/// The subject of a reply: its first line that is not blank or a code fence.
pub fn subject_of(reply: &str) -> &str {
    reply
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```"))
        .unwrap_or("")
        .trim_matches(|c| c == '"' || c == '`')
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// What an input allows, to judge replies against.
pub struct Judge {
    conventional: bool,
    /// The type of the real commit, in the Conventional set.
    real_type: Option<String>,
    /// The scopes of the history subjects.
    scopes: BTreeSet<String>,
    history: Vec<String>,
    /// The history, paths and diff, lowercase: where a specific may be from.
    known: String,
    /// The paths and diff, lowercase: where a description's words are.
    diff: String,
}

impl Judge {
    /// For the input of a case of `set` (`conventional` or `free`), whose
    /// real commit has type `real_type` if it has one.
    pub fn new(set: &str, real_type: Option<&str>, inputs: &Value) -> Self {
        let mut without = inputs.clone();
        without["subjects"] = json!([]);
        let history: Vec<String> = inputs["subjects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Self {
            conventional: set == "conventional",
            real_type: real_type.map(|t| fold_type(t).to_owned()),
            scopes: history
                .iter()
                .filter_map(|h| SCOPE.captures(h).map(|s| s[1].to_owned()))
                .collect(),
            history,
            known: evidence(inputs),
            diff: evidence(&without),
        }
    }

    fn copied(&self, subject: &str) -> bool {
        let normal = |s: &str| s.trim().trim_end_matches('.').to_lowercase();
        let exact = normal(subject);
        if self.history.iter().any(|h| normal(h) == exact) {
            return true;
        }
        let ours = words(&description(subject));
        if ours.len() < 4 {
            return false;
        }
        self.history.iter().any(|h| {
            let theirs = words(&description(h));
            ours.iter().all(|w| theirs.contains(w))
                && ours.len() as f64 >= NEAR_COPY_SHARE * theirs.len() as f64
        })
    }

    /// Why `subject` is rejected, if it is: the first check it fails, in the
    /// order of [`Failure`].
    pub fn failure(&self, subject: &str) -> Option<Failure> {
        if !subject.chars().any(char::is_alphanumeric) {
            return Some(Failure::Empty);
        }
        if self.conventional && conventional_type(subject).is_none() {
            return Some(Failure::Format);
        }
        if subject.chars().count() > SUBJECT_MAX {
            return Some(Failure::Long);
        }
        if self.copied(subject) {
            return Some(Failure::Copied);
        }
        if unsupported(subject, &self.known) {
            return Some(Failure::Unsupported);
        }
        if ungrounded(subject, &self.diff) {
            return Some(Failure::Ungrounded);
        }
        if self.conventional {
            if SCOPE
                .captures(subject)
                .is_some_and(|s| !self.scopes.contains(&s[1]))
            {
                return Some(Failure::Scope);
            }
            if self.real_type.is_some() && conventional_type(subject) != self.real_type {
                return Some(Failure::Type);
            }
        }
        None
    }
}

/// A subject without its case or final period, to tell two apart.
fn normal(subject: &str) -> String {
    subject.trim().trim_end_matches('.').to_lowercase()
}

/// The pair for one input: `replies` are what the model sampled and `target`
/// the real subject. The rejected reply is the one failing the most serious
/// check (the first, if several fail alike); `None` without one, or without
/// a subject that passes to prefer.
pub fn pair(
    judge: &Judge,
    target: &str,
    replies: &[String],
    prefer: Prefer,
) -> Option<(String, String, Failure)> {
    let subjects: Vec<&str> = replies.iter().map(|r| subject_of(r)).collect();
    let (rejected, failure) = subjects
        .iter()
        .filter_map(|s| judge.failure(s).map(|f| (*s, f)))
        .min_by_key(|(_, f)| *f)?;
    let sample = || {
        subjects
            .iter()
            .filter(|s| judge.failure(s).is_none())
            // The closest in words; the first of those equally close.
            .min_by(|a, b| f1(b, target).total_cmp(&f1(a, target)))
            .copied()
    };
    let real = || judge.failure(target).is_none().then_some(target);
    let chosen = match prefer {
        Prefer::Sample => sample().or_else(real),
        Prefer::Real => real().or_else(sample),
    }?;
    (normal(chosen) != normal(rejected)).then(|| (chosen.to_owned(), rejected.to_owned(), failure))
}

/// What came out of [`write`]: inputs seen, pairs written, and by failure.
#[derive(Debug, Default)]
pub struct Summary {
    pub inputs: usize,
    pub pairs: usize,
    pub by_failure: BTreeMap<&'static str, usize>,
    pub real_chosen: usize,
}

fn lines(path: &Path) -> Result<Vec<Value>> {
    std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .lines()
        .map(|l| {
            serde_json::from_str(l).with_context(|| format!("{}: bad JSON line", path.display()))
        })
        .collect()
}

/// Writes the pairs of `candidates` (a line per input: `system`, `user` and
/// the sampled `replies`), with the `inputs` and `meta` of the same lines,
/// to `out`, one JSON object a line.
pub fn write(
    candidates: &Path,
    inputs: &Path,
    meta: &Path,
    out: &Path,
    prefer: Prefer,
) -> Result<Summary> {
    let (candidates, inputs, meta) = (lines(candidates)?, lines(inputs)?, lines(meta)?);
    ensure!(
        candidates.len() == inputs.len() && inputs.len() == meta.len(),
        "{} candidates, {} inputs, {} meta",
        candidates.len(),
        inputs.len(),
        meta.len()
    );
    let mut summary = Summary::default();
    let mut text = String::new();
    for ((c, i), m) in candidates.iter().zip(&inputs).zip(&meta) {
        summary.inputs += 1;
        let replies: Vec<String> = c["replies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let target = m["target"].as_str().unwrap_or("");
        let judge = Judge::new(m["set"].as_str().unwrap_or(""), m["type"].as_str(), i);
        let Some((chosen, rejected, failure)) = pair(&judge, target, &replies, prefer) else {
            continue;
        };
        summary.pairs += 1;
        *summary.by_failure.entry(failure.name()).or_default() += 1;
        summary.real_chosen += usize::from(chosen == target);
        let row = json!({
            "system": c["system"],
            "user": c["user"],
            "chosen": chosen,
            "rejected": rejected,
            "failure": failure.name(),
            "source": if chosen == target { "real" } else { "sample" },
            "set": m["set"],
            "repo": m["repo"],
            "hash": m["hash"],
            "type": m["type"],
            "history": m["history"],
        });
        text.push_str(&row.to_string());
        text.push('\n');
    }
    std::fs::write(out, text).with_context(|| format!("cannot write {}", out.display()))?;
    Ok(summary)
}

#[cfg(test)]
#[path = "pairs_tests.rs"]
mod tests;
