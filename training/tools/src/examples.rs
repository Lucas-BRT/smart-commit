//! Training examples from selected inputs, and subsets of them.
//!
//! `make-examples` builds the prompts with `smart-commit prompt --inputs` and
//! an environment of nothing but an empty `XDG_CONFIG_HOME`, so only
//! smart-commit's own defaults could shape them, and writes `examples.jsonl`:
//! `{"system", "user", "target", "repo", "hash", "set"}` per line. `--rules`,
//! `--diff` and `--scopes` are always passed, never left to smart-commit's defaults: the
//! default file list became `annotated` after every model was trained on the
//! `plain` one, and a data set built with it would not have said so.
//!
//! `nest` writes nested subsets for a learning curve: each size holds every
//! example of the smaller ones, ranked by a salted SHA-256 of the commit hash
//! (`select` ranked by the plain hash and took the lowest of pools of very
//! different sizes, so the plain ranking would put one pool first).

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Map, Value};

use crate::pyjson;
use crate::pytext::read_lines as lines_of;

/// Writes `DIR/examples.jsonl` from `DIR/inputs.jsonl` and `DIR/meta.jsonl`;
/// how many examples.
pub fn make_examples(
    dir: &Path,
    binary: &Path,
    rules: &str,
    diff: &str,
    scopes: &str,
) -> Result<usize> {
    let config_home = tempfile::TempDir::new()?;
    let done = Command::new(binary)
        .env_clear()
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("PATH", "/usr/bin:/bin")
        .args(["prompt", "--inputs"])
        .arg(dir.join("inputs.jsonl"))
        .args(["--rules", rules, "--diff", diff, "--scopes", scopes])
        .output()
        .with_context(|| format!("cannot run {}", binary.display()))?;
    if !done.status.success() {
        bail!(
            "{} prompt failed ({}):\n{}",
            binary.display(),
            done.status,
            String::from_utf8_lossy(&done.stderr)
        );
    }
    let prompts: Vec<String> = String::from_utf8(done.stdout)?
        .lines()
        .map(str::to_owned)
        .collect();
    let meta = lines_of(&dir.join("meta.jsonl"))?;
    ensure!(
        prompts.len() == meta.len(),
        "{} prompts for {} cases",
        prompts.len(),
        meta.len()
    );
    let mut out = String::new();
    for (prompt, meta) in prompts.iter().zip(&meta) {
        let (prompt, meta): (Value, Value) =
            (serde_json::from_str(prompt)?, serde_json::from_str(meta)?);
        let mut example: Map<String, Value> = prompt
            .as_object()
            .context("a prompt that is not an object")?
            .clone();
        // The sanitizer drops a trailing period; train on what it keeps.
        let target = meta["target"]
            .as_str()
            .unwrap_or("")
            .trim_end_matches('.')
            .trim_end();
        example.insert("target".into(), target.into());
        for key in ["repo", "hash", "set"] {
            example.insert(key.into(), meta[key].clone());
        }
        out.push_str(&pyjson::dumps(&Value::Object(example)));
        out.push('\n');
    }
    std::fs::write(dir.join("examples.jsonl"), out)?;
    Ok(prompts.len())
}

/// Writes `PREFIX-N.jsonl` for each size: the first N examples of `src` in
/// the order of the salted hash; a line each of what it wrote.
pub fn nest(src: &Path, prefix: &str, sizes: &[usize]) -> Result<Vec<String>> {
    let mut ranked: Vec<(String, String, bool)> = Vec::new();
    for line in lines_of(src)? {
        let example: Value = serde_json::from_str(&line)?;
        let hash = example["hash"]
            .as_str()
            .context("an example without a hash")?;
        let rank = smart_commit::models::sha256_hex(format!("nest:{hash}").as_bytes());
        ranked.push((rank, line, example["set"] == "conventional"));
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0));
    let mut written = Vec::new();
    for &n in sizes {
        ensure!(n <= ranked.len(), "{n} examples asked of {}", ranked.len());
        let path = format!("{prefix}-{n}.jsonl");
        std::fs::write(
            &path,
            ranked[..n]
                .iter()
                .map(|(_, line, _)| format!("{line}\n"))
                .collect::<String>(),
        )?;
        let conventional = ranked[..n].iter().filter(|(_, _, c)| *c).count();
        written.push(format!("{path}: {n} examples, {conventional} conventional"));
    }
    Ok(written)
}

#[cfg(test)]
#[path = "examples_tests.rs"]
mod tests;
