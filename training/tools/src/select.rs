//! Deterministic subsets of `build-inputs` output.
//!
//! Commits are ranked by the SHA-256 of their hash, so the choice does not
//! depend on the dataset's order; at most `--per-repo` per repository keeps
//! one large repository from dominating.
//!
//! `--releases` reads a source made by `build-inputs --releases`, whose
//! targets are the release bumps the target filters would otherwise drop.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use crate::build_inputs::keep_target;
use crate::pyjson;
use crate::score::{evidence, unsupported};

pub struct Options {
    pub src: PathBuf,
    pub out: PathBuf,
    pub conventional: usize,
    pub free: usize,
    pub per_repo: usize,
    pub min_history: usize,
    pub max_history: Option<usize>,
    pub supported_only: bool,
    pub max_type_share: f64,
    pub releases: bool,
    /// Selections whose commits (by hash) are left out, e.g. what a model was trained on.
    pub exclude_hashes_of: Vec<PathBuf>,
    /// Selections whose repositories are left out, e.g. the evaluation sets.
    pub exclude_repos_of: Vec<PathBuf>,
}

/// Counts in the order keys were first seen, like `collections.Counter`.
#[derive(Default)]
struct Counter<K: PartialEq>(Vec<(K, usize)>);

impl<K: PartialEq + Clone> Counter<K> {
    fn get(&self, key: &K) -> usize {
        self.0.iter().find(|(k, _)| k == key).map_or(0, |(_, n)| *n)
    }

    fn add(&mut self, key: &K) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, n)) => *n += 1,
            None => self.0.push((key.clone(), 1)),
        }
    }

    fn most_common(&self) -> Vec<&(K, usize)> {
        let mut all: Vec<_> = self.0.iter().collect();
        all.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        all
    }
}

fn lines_of(path: &Path) -> Result<Vec<String>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(lines)
}

fn rank(hash: &str) -> String {
    smart_commit::models::sha256_hex(hash.as_bytes())
}

/// The values of `key` in the `meta.jsonl` of each directory.
fn meta_values(dirs: &[PathBuf], key: &str) -> Result<HashSet<String>> {
    let mut values = HashSet::new();
    for dir in dirs {
        for line in lines_of(&dir.join("meta.jsonl"))? {
            let meta: Value = serde_json::from_str(&line)?;
            values.extend(meta[key].as_str().map(str::to_owned));
        }
    }
    Ok(values)
}

pub fn select(options: &Options) -> Result<()> {
    let excluded_hashes = meta_values(&options.exclude_hashes_of, "hash")?;
    let excluded_repos = meta_values(&options.exclude_repos_of, "repo")?;
    let meta: Vec<Value> = lines_of(&options.src.join("meta.jsonl"))?
        .iter()
        .map(|l| serde_json::from_str(l))
        .collect::<Result<_, _>>()?;
    // inputs.jsonl can be larger than memory: index line offsets, read lines on demand.
    let inputs_path = options.src.join("inputs.jsonl");
    let mut offsets = Vec::new();
    let mut reader = BufReader::new(File::open(&inputs_path)?);
    let mut position = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        offsets.push(position);
        position += n as u64;
    }
    if offsets.len() != meta.len() {
        bail!("{} inputs for {} meta lines", offsets.len(), meta.len());
    }
    let mut inputs_file = File::open(&inputs_path)?;
    let mut input_line = |i: usize| -> Result<String> {
        inputs_file.seek(SeekFrom::Start(offsets[i]))?;
        let mut text = String::new();
        BufReader::new(&mut inputs_file).read_line(&mut text)?;
        Ok(text.trim_end_matches('\n').to_owned())
    };

    let text = |i: usize, key: &str| meta[i][key].as_str().unwrap_or("").to_owned();
    let kind_of = |i: usize| meta[i]["type"].as_str().map(str::to_owned);
    let mut share: BTreeMap<String, Vec<bool>> = BTreeMap::new();
    for m in &meta {
        share
            .entry(m["repo"].as_str().unwrap_or("").to_owned())
            .or_default()
            .push(!m["type"].is_null());
    }
    let conventional_repo: BTreeMap<&String, bool> = share
        .iter()
        .map(|(r, v)| {
            (
                r,
                v.iter().filter(|b| **b).count() as f64 / v.len() as f64 >= 0.5,
            )
        })
        .collect();

    let (mut conventional, mut free) = (Vec::new(), Vec::new());
    let mut dropped = 0usize;
    for (i, m) in meta.iter().enumerate() {
        let history = m["history"].as_u64().unwrap_or(0) as usize;
        if history < options.min_history || options.max_history.is_some_and(|max| history > max) {
            continue;
        }
        // After the repositories' style is known: only the pool shrinks.
        if excluded_hashes.contains(&text(i, "hash")) || excluded_repos.contains(&text(i, "repo")) {
            continue;
        }
        // Filters added after the inputs were built apply here too.
        if !options.releases && !keep_target(&text(i, "target")) {
            dropped += 1;
            continue;
        }
        if conventional_repo[&text(i, "repo")] {
            if !m["type"].is_null() {
                conventional.push(i);
            }
        } else {
            free.push(i);
        }
    }

    let mut chosen: Vec<(&str, usize)> = Vec::new();
    for (kind, want, mut pool) in [
        ("conventional", options.conventional, conventional),
        ("free", options.free, free),
    ] {
        let mut per_repo: Counter<String> = Counter::default();
        let mut per_type: Counter<Option<String>> = Counter::default();
        let type_cap = if kind == "conventional" {
            (want as f64 * options.max_type_share) as usize
        } else {
            want
        };
        let mut taken = 0;
        pool.sort_by_cached_key(|&i| rank(&text(i, "hash")));
        for i in pool {
            if taken == want {
                break;
            }
            let repo = text(i, "repo");
            let kind_of_i = kind_of(i);
            if per_repo.get(&repo) < options.per_repo && per_type.get(&kind_of_i) < type_cap {
                // Checked last: it reads the inputs.
                if options.supported_only {
                    let inputs: Value = serde_json::from_str(&input_line(i)?)?;
                    if unsupported(&text(i, "target"), &evidence(&inputs)) {
                        dropped += 1;
                        continue;
                    }
                }
                per_repo.add(&repo);
                per_type.add(&kind_of_i);
                chosen.push((kind, i));
                taken += 1;
            }
        }
        eprintln!("{kind}: {taken} commits from {} repos", per_repo.0.len());
        eprintln!("  {dropped} dropped by the target filters so far");
        if kind == "conventional" {
            let types: Vec<String> = per_type
                .most_common()
                .iter()
                .map(|(k, n)| match k {
                    Some(k) => format!("'{k}': {n}"),
                    None => format!("None: {n}"),
                })
                .collect();
            eprintln!("  types: {{{}}}", types.join(", "));
        }
    }

    std::fs::create_dir_all(&options.out)?;
    let mut inputs_out = BufWriter::new(File::create(options.out.join("inputs.jsonl"))?);
    let mut meta_out = BufWriter::new(File::create(options.out.join("meta.jsonl"))?);
    for (kind, i) in chosen {
        writeln!(inputs_out, "{}", input_line(i)?)?;
        let mut line = meta[i].clone();
        line["set"] = json!(kind);
        writeln!(meta_out, "{}", pyjson::dumps(&line))?;
    }
    inputs_out.flush()?;
    meta_out.flush()?;
    Ok(())
}

/// Drops, from a selection already made, the commits a cleaner label set
/// leaves out: subjects `keep_target` now rejects (versions alone, release
/// bumps) and subjects naming a version, number or identifier found neither
/// in the diff nor in the history (`unsupported`). Unlike `select
/// --supported-only`, nothing else is chosen in their place, so the result
/// differs from its source only by the filter: how h3a was made from h1.
pub fn filter_supported(src: &Path, out: &Path) -> Result<()> {
    let inputs = lines_of(&src.join("inputs.jsonl"))?;
    let meta = lines_of(&src.join("meta.jsonl"))?;
    if inputs.len() != meta.len() {
        bail!("{} inputs for {} meta lines", inputs.len(), meta.len());
    }
    let mut kept = Vec::new();
    for (i, m) in inputs.iter().zip(&meta) {
        let parsed: Value = serde_json::from_str(m)?;
        let target = parsed["target"].as_str().unwrap_or("");
        if keep_target(target) && !unsupported(target, &evidence(&serde_json::from_str(i)?)) {
            kept.push((i, m, parsed));
        }
    }
    std::fs::create_dir_all(out)?;
    let write = |name: &str, lines: Vec<&String>| -> Result<()> {
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(out.join(name), text)?;
        Ok(())
    };
    write("inputs.jsonl", kept.iter().map(|(i, _, _)| *i).collect())?;
    write("meta.jsonl", kept.iter().map(|(_, m, _)| *m).collect())?;
    let conventional = kept
        .iter()
        .filter(|(_, _, m)| m["set"] == "conventional")
        .count();
    eprintln!(
        "{} -> {} commits, {conventional} conventional",
        inputs.len(),
        kept.len()
    );
    Ok(())
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
