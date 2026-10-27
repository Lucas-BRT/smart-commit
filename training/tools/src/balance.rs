//! The set of pairs for DPO, balanced and split: what `pairs` writes is as
//! lopsided as the inputs, and a model trained on it learns the repositories
//! that have the most failures.
//!
//! - The validation set is every pair of about a tenth of the repositories
//!   (by a hash of the repository, so it does not depend on the order or on
//!   the other files): the training loss says nothing about repositories the
//!   model has not seen.
//! - The training set has at most `max_per_repo` pairs per repository, at
//!   most `max_type_share` of a bucket in one real type, and at most a
//!   quota of pairs per bucket: Conventional commits, young repositories
//!   (fewer than ten commits of history) and the free set. Within a bucket
//!   the pairs are taken in the order of a hash of the commit.
//! - With `match_marginals`, pairs are dropped until the chosen and the
//!   rejected replies have the same distribution of type and the same share
//!   with a scope. The rejected reply is a sample of the model, which errs
//!   most in the types and scopes it writes most, so the pairs add up to
//!   "fewer `fix`, more `chore`, no scope": the cheapest way to lower the DPO
//!   loss is to move the model's habit, and a run on such pairs collapsed to
//!   `chore`. With the marginals equal only what depends on the diff can.
//! - A repeated pair is dropped. So is, if asked, a pair whose replies
//!   differ in length by more than `max_length_ratio`, unless the reply was
//!   rejected for being too long. It is off by default: the pairs of the
//!   first run were already even (chosen longer in 51%, 37.7 columns against
//!   38.2), and a ratio of 1.6 left the chosen 4.6 columns shorter, since a
//!   vague reply is shorter than the specific one it loses to. The summary
//!   says how even the set is.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::build_inputs::conventional_type;
use crate::score::SCOPE;

pub struct Options {
    pub pairs: Vec<PathBuf>,
    pub out: PathBuf,
    /// The share of the repositories whose pairs are the validation set.
    pub val_share: f64,
    pub max_per_repo: usize,
    pub max_type_share: f64,
    pub max_length_ratio: f64,
    pub conventional: usize,
    pub young: usize,
    pub free: usize,
    /// Drop pairs until the chosen and the rejected replies have the same
    /// distribution of type and of scope, within `marginal_tolerance`.
    pub match_marginals: bool,
    pub marginal_tolerance: f64,
}

/// Fewer commits of history than this is a young repository.
const YOUNG: u64 = 10;

fn digest(text: &str) -> Vec<u8> {
    Sha256::digest(text.as_bytes()).to_vec()
}

fn text<'a>(pair: &'a Value, key: &str) -> &'a str {
    pair[key].as_str().unwrap_or("")
}

/// `conventional`, `young` or `free`.
fn bucket(pair: &Value) -> &'static str {
    if pair["history"].as_u64().unwrap_or(YOUNG) < YOUNG {
        "young"
    } else if text(pair, "set") == "conventional" {
        "conventional"
    } else {
        "free"
    }
}

fn in_validation(pair: &Value, share: f64) -> bool {
    let byte = digest(&format!("val:{}", text(pair, "repo")))[0];
    f64::from(byte) < share * 256.0
}

fn balanced_length(pair: &Value, ratio: f64) -> bool {
    if text(pair, "failure") == "long" {
        return true;
    }
    let (a, b) = (
        text(pair, "chosen").chars().count() as f64,
        text(pair, "rejected").chars().count() as f64,
    );
    a.max(b) <= ratio * a.min(b).max(1.0)
}

/// What [`balance`] wrote.
#[derive(Debug, Default)]
pub struct Summary {
    pub read: usize,
    pub dropped_length: usize,
    pub dropped_repeated: usize,
    pub validation: usize,
    pub train: usize,
    pub train_repos: usize,
    pub by_bucket: BTreeMap<&'static str, usize>,
    pub by_type: BTreeMap<String, usize>,
    pub by_failure: BTreeMap<String, usize>,
    pub real_chosen: usize,
    pub chosen_chars: usize,
    pub rejected_chars: usize,
    pub chosen_longer: usize,
    /// Pairs dropped to match the marginals, and the imbalance before and after.
    pub matched_removed: usize,
    pub imbalance_before: i64,
    pub imbalance_after: i64,
}

/// The key of the share with a scope among the types (no type has a NUL in it).
const SCOPE_KEY: &str = "\u{0}scope";

/// What a pair says about the type and the scope of its two replies.
struct Marks {
    chosen_type: Option<String>,
    rejected_type: Option<String>,
    chosen_scope: bool,
    rejected_scope: bool,
}

fn marks(pair: &Value) -> Marks {
    Marks {
        chosen_type: conventional_type(text(pair, "chosen")),
        rejected_type: conventional_type(text(pair, "rejected")),
        chosen_scope: SCOPE.is_match(text(pair, "chosen")),
        rejected_scope: SCOPE.is_match(text(pair, "rejected")),
    }
}

/// How many more replies of each type the chosen side has than the rejected
/// side (the key `scope` for those with a scope), over the pairs kept.
#[derive(Clone, Default)]
struct Imbalance(BTreeMap<String, i64>);

impl Imbalance {
    fn add(&mut self, marks: &Marks, sign: i64) {
        let mut bump = |key: &str, by: i64| *self.0.entry(key.to_owned()).or_default() += by;
        if let Some(kind) = &marks.chosen_type {
            bump(kind, sign);
        }
        if let Some(kind) = &marks.rejected_type {
            bump(kind, -sign);
        }
        bump(
            SCOPE_KEY,
            sign * (i64::from(marks.chosen_scope) - i64::from(marks.rejected_scope)),
        );
    }

    fn total(&self) -> i64 {
        self.0.values().map(|d| d.abs()).sum()
    }

    /// How much the total would change if a pair with these marks left: only
    /// the (at most three) values it touches count, so nothing is copied.
    fn change_if_removed(&self, marks: &Marks) -> i64 {
        fn bump<'a>(touched: &mut Vec<(&'a str, i64)>, key: &'a str, by: i64) {
            match touched.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += by,
                None => touched.push((key, by)),
            }
        }
        let mut touched = Vec::with_capacity(3);
        if let Some(kind) = &marks.chosen_type {
            bump(&mut touched, kind, -1);
        }
        if let Some(kind) = &marks.rejected_type {
            bump(&mut touched, kind, 1);
        }
        bump(
            &mut touched,
            SCOPE_KEY,
            i64::from(marks.rejected_scope) - i64::from(marks.chosen_scope),
        );
        touched
            .into_iter()
            .map(|(key, by)| {
                let now = self.0.get(key).copied().unwrap_or(0);
                (now + by).abs() - now.abs()
            })
            .sum()
    }
}

/// Drops, one at a time, the pair whose departure brings the chosen and the
/// rejected replies closest in type and scope, until the imbalance is at most
/// `tolerance` per pair or no pair helps. Returns what is kept, in order.
fn match_marginals(pairs: Vec<&Value>, tolerance: f64) -> (Vec<&Value>, i64, i64) {
    let marks: Vec<Marks> = pairs.iter().map(|p| marks(p)).collect();
    let mut imbalance = Imbalance::default();
    marks.iter().for_each(|m| imbalance.add(m, 1));
    let before = imbalance.total();
    let mut kept = vec![true; pairs.len()];
    let mut left = pairs.len();
    while imbalance.total() as f64 > tolerance * left as f64 {
        let best = (0..pairs.len())
            .filter(|&i| kept[i])
            .map(|i| (imbalance.change_if_removed(&marks[i]), i))
            .min_by_key(|&(change, i)| (change, i));
        match best {
            Some((change, i)) if change < 0 => {
                imbalance.add(&marks[i], -1);
                kept[i] = false;
                left -= 1;
            }
            _ => break,
        }
    }
    let after = imbalance.total();
    let pairs = pairs
        .into_iter()
        .zip(kept)
        .filter_map(|(p, k)| k.then_some(p))
        .collect();
    (pairs, before, after)
}

pub fn balance(options: &Options) -> Result<Summary> {
    let mut summary = Summary::default();
    let mut pairs = Vec::new();
    for path in &options.pairs {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        for line in content.lines() {
            let pair: Value = serde_json::from_str(line)
                .with_context(|| format!("{}: bad JSON line", path.display()))?;
            summary.read += 1;
            if balanced_length(&pair, options.max_length_ratio) {
                pairs.push(pair);
            } else {
                summary.dropped_length += 1;
            }
        }
    }
    // The order of the files and of the lines must not matter: not for what is
    // taken first, and not for which of two repeated pairs is the one kept, so
    // the repeats are dropped after this sort and not while reading.
    pairs.sort_by_cached_key(|p| {
        (
            digest(&format!("{}:{}", text(p, "hash"), text(p, "rejected"))),
            p.to_string(),
        )
    });
    let mut seen = HashSet::new();
    pairs.retain(|p| {
        let new = seen.insert((text(p, "chosen").to_owned(), text(p, "rejected").to_owned()));
        summary.dropped_repeated += usize::from(!new);
        new
    });

    let mut pool: BTreeMap<&str, usize> = BTreeMap::new();
    for pair in pairs
        .iter()
        .filter(|p| !in_validation(p, options.val_share))
    {
        *pool.entry(bucket(pair)).or_default() += 1;
    }
    let quota = |name: &str| match name {
        "conventional" => options.conventional,
        "young" => options.young,
        _ => options.free,
    };
    let mut per_repo: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_bucket_type: BTreeMap<(&str, String), usize> = BTreeMap::new();
    let mut taken: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut selected, mut validation) = (Vec::new(), String::new());
    for pair in &pairs {
        if in_validation(pair, options.val_share) {
            validation.push_str(&format!("{pair}\n"));
            summary.validation += 1;
            continue;
        }
        let name = bucket(pair);
        let kind = pair["type"].as_str().unwrap_or("").to_owned();
        // Of what the bucket will hold: its quota, or all it has.
        let size = quota(name).min(pool[name]);
        let type_cap = (options.max_type_share * size as f64).ceil() as usize;
        let repo = text(pair, "repo").to_owned();
        if taken.get(name).copied().unwrap_or(0) >= quota(name)
            || per_repo.get(&repo).copied().unwrap_or(0) >= options.max_per_repo
            || (!kind.is_empty()
                && per_bucket_type
                    .get(&(name, kind.clone()))
                    .copied()
                    .unwrap_or(0)
                    >= type_cap)
        {
            continue;
        }
        *per_repo.entry(repo).or_default() += 1;
        *per_bucket_type.entry((name, kind)).or_default() += 1;
        *taken.entry(name).or_default() += 1;
        selected.push(pair);
    }
    let selected = if options.match_marginals {
        let count = selected.len();
        let (kept, before, after) = match_marginals(selected, options.marginal_tolerance);
        summary.matched_removed = count - kept.len();
        (summary.imbalance_before, summary.imbalance_after) = (before, after);
        kept
    } else {
        selected
    };
    let (mut train, mut repos) = (String::new(), BTreeSet::new());
    for pair in selected {
        let name = bucket(pair);
        let kind = pair["type"].as_str().unwrap_or("").to_owned();
        *summary.by_bucket.entry(name).or_default() += 1;
        *summary
            .by_failure
            .entry(text(pair, "failure").to_owned())
            .or_default() += 1;
        if !kind.is_empty() {
            *summary.by_type.entry(kind).or_default() += 1;
        }
        summary.real_chosen += usize::from(text(pair, "source") == "real");
        let (chosen, rejected) = (
            text(pair, "chosen").chars().count(),
            text(pair, "rejected").chars().count(),
        );
        summary.chosen_chars += chosen;
        summary.rejected_chars += rejected;
        summary.chosen_longer += usize::from(chosen > rejected);
        summary.train += 1;
        repos.insert(text(pair, "repo").to_owned());
        train.push_str(&format!("{pair}\n"));
    }
    summary.train_repos = repos.len();
    std::fs::create_dir_all(&options.out)?;
    std::fs::write(options.out.join("train.jsonl"), train)?;
    std::fs::write(options.out.join("val.jsonl"), validation)?;
    Ok(summary)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

impl Summary {
    pub fn json(&self) -> Value {
        json!({
            "read": self.read,
            "dropped_length": self.dropped_length,
            "dropped_repeated": self.dropped_repeated,
            "validation": self.validation,
            "train": self.train,
            "train_repos": self.train_repos,
            "by_bucket": self.by_bucket,
            "by_type": self.by_type,
            "by_failure": self.by_failure,
            "real_chosen": self.real_chosen,
            "matched_removed": self.matched_removed,
            "imbalance_before": self.imbalance_before,
            "imbalance_after": self.imbalance_after,
            "mean_chosen_chars": round1(self.chosen_chars as f64 / self.train.max(1) as f64),
            "mean_rejected_chars": round1(self.rejected_chars as f64 / self.train.max(1) as f64),
            "chosen_longer_share": round1(100.0 * self.chosen_longer as f64 / self.train.max(1) as f64) / 100.0,
        })
    }
}

#[cfg(test)]
#[path = "balance_tests.rs"]
mod tests;
