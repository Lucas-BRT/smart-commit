//! Builds the commit scenarios of `scenarios/` (see `scenarios.rs`).
//!
//! `build` writes DIR/inputs.jsonl and DIR/meta.jsonl, the layout of
//! `select`'s output: `make_examples.py DIR SMART_COMMIT` makes training
//! examples of them, `smart-commit eval DIR/inputs.jsonl` and `score DIR`
//! evaluate a model on them. A case whose outcome differs from the expected
//! one (smart-commit refuses to generate, or generates when it should
//! refuse) fails the build, unless the case documents it as a known `issue`.
//!
//! `show` leaves a base repository, or a case applied on it, in DIR to look
//! at while writing cases.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Result, anyhow, bail};

use crate::pyjson;
use crate::scenarios::{
    Case, Repo, apply, build_repo, capture, load_cases, load_repos, meta_of, select,
};

/// Where the scenarios are, in a checkout.
pub fn default_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../scenarios")
}

fn repo<'a>(repos: &'a [Repo], case: &Case) -> &'a Repo {
    repos
        .iter()
        .find(|r| r.name == case.repo)
        .expect("validated: the repo exists")
}

/// Returns how many cases failed.
pub fn build(root: &Path, out: &Path, binary: &str, split: &str, only: &str) -> Result<usize> {
    let repos = load_repos(root).map_err(|e| anyhow!("{e}"))?;
    let all = load_cases(root, &repos).map_err(|e| anyhow!("{e}"))?;
    let cases = select(&all, split, only);
    std::fs::create_dir_all(out)?;
    let mut inputs_file = BufWriter::new(File::create(out.join("inputs.jsonl"))?);
    let mut meta_file = BufWriter::new(File::create(out.join("meta.jsonl"))?);
    let mut failures = 0;
    for case in &cases {
        let tmp = tempfile::TempDir::new()?;
        let workdir = tmp.path().join("repo");
        apply(case, repo(&repos, case), &workdir).map_err(|e| anyhow!("{e}"))?;
        let captured = capture(&workdir, binary);
        let refused = captured.is_err();
        if refused != (case.outcome == "error") {
            let what = match &captured {
                Err(error) => format!("refused: {error}"),
                Ok(_) => "generated a message".to_owned(),
            };
            if case.issue.is_empty() {
                eprintln!("FAILED       {}: {what}", case.id);
                failures += 1;
            } else {
                eprintln!("known issue  {}: {what} ({})", case.id, case.issue);
            }
        }
        let Ok(inputs) = captured else { continue };
        writeln!(inputs_file, "{}", pyjson::dumps(&inputs))?;
        writeln!(
            meta_file,
            "{}",
            pyjson::dumps(&meta_of(case, repo(&repos, case), &inputs))
        )?;
    }
    inputs_file.flush()?;
    meta_file.flush()?;
    eprintln!("{} cases, {failures} failed", cases.len());
    Ok(failures)
}

pub fn show(root: &Path, out: &Path, repo_name: Option<&str>, case_id: Option<&str>) -> Result<()> {
    let repos = load_repos(root).map_err(|e| anyhow!("{e}"))?;
    if let Some(id) = case_id {
        let cases = load_cases(root, &repos).map_err(|e| anyhow!("{e}"))?;
        let Some(case) = cases.iter().find(|c| c.id == id) else {
            bail!("no case {id}");
        };
        apply(case, repo(&repos, case), out).map_err(|e| anyhow!("{e}"))?;
    } else {
        let name = repo_name.expect("one of --repo and --case");
        let Some(found) = repos.iter().find(|r| r.name == name) else {
            bail!("no repo {name}");
        };
        build_repo(found, out, true).map_err(|e| anyhow!("{e}"))?;
    }
    println!("{}", out.display());
    Ok(())
}

pub fn list(root: &Path) -> Result<()> {
    let repos = load_repos(root).map_err(|e| anyhow!("{e}"))?;
    let cases = load_cases(root, &repos).map_err(|e| anyhow!("{e}"))?;
    let count = |level: i64, split: &str| {
        cases
            .iter()
            .filter(|c| c.level == level && c.split == split)
            .count()
    };
    println!("level  train  eval");
    let levels: BTreeSet<i64> = cases.iter().map(|c| c.level).collect();
    for level in levels {
        println!(
            "{level:>5}  {:>5}  {:>4}",
            count(level, "train"),
            count(level, "eval")
        );
    }
    let total = |split: &str| cases.iter().filter(|c| c.split == split).count();
    println!("total  {:>5}  {:>4}", total("train"), total("eval"));
    for case in &cases {
        let flags: Vec<&str> = [
            if case.outcome == "message" {
                ""
            } else {
                case.outcome.as_str()
            },
            if case.issue.is_empty() { "" } else { "issue" },
        ]
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect();
        let line = format!(
            "  L{} {:<5} {:<34} {} {}",
            case.level,
            case.split,
            case.id,
            case.title,
            flags.join(" ")
        );
        println!("{}", line.trim_end());
    }
    Ok(())
}
