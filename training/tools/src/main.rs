//! The data tools for training smart-commit's models. They are a crate of
//! their own so that what they need (parquet, above all) never reaches the
//! binary users install.

mod balance;
mod build_inputs;
mod build_scenarios;
mod mt;
mod pairs;
mod pyjson;
mod pytext;
mod scenarios;
mod score;
mod select;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sc-training",
    about = "The data tools for training smart-commit's models"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// CommitChronicle parquet shards -> `inputs.jsonl` and `meta.jsonl` in OUT.
    BuildInputs {
        out: PathBuf,
        #[arg(required = true)]
        shards: Vec<PathBuf>,
        /// Branch in every input [default: null].
        #[arg(long)]
        branch: Option<String>,
        /// Keep only the release bumps whose version the diff adds.
        #[arg(long)]
        releases: bool,
    },
    /// Deterministic subsets of `build-inputs` output, by a hash of the commit.
    Select {
        src: PathBuf,
        out: PathBuf,
        /// Commits with a Conventional subject from mostly Conventional repos.
        #[arg(long)]
        conventional: usize,
        /// Commits from repos that are mostly not Conventional.
        #[arg(long)]
        free: usize,
        #[arg(long, default_value_t = 40)]
        per_repo: usize,
        #[arg(long, default_value_t = 10)]
        min_history: usize,
        /// E.g. 5 for young repositories, which the default leaves out.
        #[arg(long)]
        max_history: Option<usize>,
        /// Drop commits whose real subject names a version, number or
        /// identifier absent from the diff and the history.
        #[arg(long)]
        supported_only: bool,
        /// At most this share of the conventional commits per type.
        #[arg(long, default_value_t = 1.0)]
        max_type_share: f64,
        /// The source is `build-inputs --releases`: keep its release bumps.
        #[arg(long)]
        releases: bool,
        /// Leave out the commits of these selections (what a model trained on).
        #[arg(long, num_args = 1..)]
        exclude_hashes_of: Vec<PathBuf>,
        /// Leave out the repositories of these selections (the evaluation sets).
        #[arg(long, num_args = 1..)]
        exclude_repos_of: Vec<PathBuf>,
    },
    /// Drops from a selection the commits a cleaner label set leaves out.
    FilterSupported { src: PathBuf, out: PathBuf },
    /// Preference pairs for DPO from the replies `mine_pairs.py` sampled.
    Pairs {
        /// A line per input: `system`, `user` and the sampled `replies`.
        candidates: PathBuf,
        /// The `inputs.jsonl` and `meta.jsonl` of those inputs.
        data: PathBuf,
        /// Where the pairs go, one JSON object a line.
        out: PathBuf,
        /// What to prefer when the real subject and samples both pass.
        #[arg(long, value_enum, default_value_t = pairs::Prefer::Sample)]
        prefer: pairs::Prefer,
    },
    /// Balances and splits the pairs `pairs` made: a validation set of whole
    /// repositories, and caps per repository, type and bucket.
    Balance {
        /// One or more files of pairs.
        #[arg(required = true)]
        pairs: Vec<PathBuf>,
        /// Where `train.jsonl` and `val.jsonl` go.
        #[arg(long)]
        out: PathBuf,
        /// The share of the repositories kept apart for validation.
        #[arg(long, default_value_t = 0.1)]
        val_share: f64,
        #[arg(long, default_value_t = usize::MAX)]
        max_per_repo: usize,
        /// At most this share of a bucket in one real type.
        #[arg(long, default_value_t = 1.0)]
        max_type_share: f64,
        /// Drop pairs whose replies differ in length by more than this
        /// factor, but those rejected for being too long. Off by default:
        /// it leaves the chosen replies shorter than the rejected.
        #[arg(long, default_value_t = f64::MAX)]
        max_length_ratio: f64,
        /// At most this many pairs of Conventional repositories.
        #[arg(long, default_value_t = usize::MAX)]
        conventional: usize,
        /// At most this many of young repositories (under ten commits).
        #[arg(long, default_value_t = usize::MAX)]
        young: usize,
        /// At most this many of the free set.
        #[arg(long, default_value_t = usize::MAX)]
        free: usize,
    },
    /// Scores `smart-commit eval` traces against the real subjects.
    Score {
        eval_dir: PathBuf,
        traces: PathBuf,
        #[arg(long)]
        json: bool,
        /// Compare with these traces of the same cases, case by case.
        #[arg(long, value_name = "TRACES")]
        against: Option<PathBuf>,
    },
    /// The commit scenarios of `training/scenarios`.
    #[command(subcommand)]
    Scenarios(ScenarioCommand),
}

#[derive(Debug, Subcommand)]
enum ScenarioCommand {
    /// Writes DIR/inputs.jsonl and DIR/meta.jsonl from the cases.
    Build {
        out: PathBuf,
        /// The smart-commit binary that records each case's inputs.
        binary: String,
        #[arg(long, default_value = "all", value_parser = ["train", "eval", "all"])]
        split: String,
        /// Only the cases whose id starts with this.
        #[arg(long, default_value = "")]
        only: String,
        /// The scenarios directory [default: training/scenarios].
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Leaves a base repository, or a case applied on it, in DIR.
    Show {
        out: PathBuf,
        #[arg(long, conflicts_with = "case", required_unless_present = "case")]
        repo: Option<String>,
        #[arg(long)]
        case: Option<String>,
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// The cases by level and split.
    List {
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::BuildInputs {
            out,
            shards,
            branch,
            releases,
        } => {
            let stats = build_inputs::build(&build_inputs::Options {
                out,
                shards,
                branch,
                releases,
            })?;
            eprintln!("{}", stats.json());
            Ok(())
        }
        Command::Select {
            src,
            out,
            conventional,
            free,
            per_repo,
            min_history,
            max_history,
            supported_only,
            max_type_share,
            releases,
            exclude_hashes_of,
            exclude_repos_of,
        } => select::select(&select::Options {
            src,
            out,
            conventional,
            free,
            per_repo,
            min_history,
            max_history,
            supported_only,
            max_type_share,
            releases,
            exclude_hashes_of,
            exclude_repos_of,
        }),
        Command::FilterSupported { src, out } => select::filter_supported(&src, &out),
        Command::Pairs {
            candidates,
            data,
            out,
            prefer,
        } => {
            let summary = pairs::write(
                &candidates,
                &data.join("inputs.jsonl"),
                &data.join("meta.jsonl"),
                &out,
                prefer,
            )?;
            eprintln!(
                "{} pairs from {} inputs ({} with the real subject chosen); by failure: {:?}",
                summary.pairs, summary.inputs, summary.real_chosen, summary.by_failure
            );
            Ok(())
        }
        Command::Balance {
            pairs,
            out,
            val_share,
            max_per_repo,
            max_type_share,
            max_length_ratio,
            conventional,
            young,
            free,
        } => {
            let summary = balance::balance(&balance::Options {
                pairs,
                out,
                val_share,
                max_per_repo,
                max_type_share,
                max_length_ratio,
                conventional,
                young,
                free,
            })?;
            eprintln!("{}", summary.json());
            Ok(())
        }
        Command::Score {
            eval_dir,
            traces,
            json,
            against,
        } => score_command(&eval_dir, &traces, json, against.as_deref()),
        Command::Scenarios(command) => {
            let root = |given: Option<PathBuf>| given.unwrap_or_else(build_scenarios::default_root);
            match command {
                ScenarioCommand::Build {
                    out,
                    binary,
                    split,
                    only,
                    root: r,
                } => {
                    let failures = build_scenarios::build(&root(r), &out, &binary, &split, &only)?;
                    if failures > 0 {
                        std::process::exit(1);
                    }
                    Ok(())
                }
                ScenarioCommand::Show {
                    out,
                    repo,
                    case,
                    root: r,
                } => build_scenarios::show(&root(r), &out, repo.as_deref(), case.as_deref()),
                ScenarioCommand::List { root: r } => build_scenarios::list(&root(r)),
            }
        }
    }
}

fn jsonl(path: &std::path::Path) -> Result<Vec<serde_json::Value>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    text.split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}

fn score_command(
    eval_dir: &std::path::Path,
    traces: &std::path::Path,
    json: bool,
    against: Option<&std::path::Path>,
) -> Result<()> {
    let meta = jsonl(&eval_dir.join("meta.jsonl"))?;
    let traces_data = jsonl(traces)?;
    anyhow::ensure!(
        meta.len() == traces_data.len(),
        "{} cases, {} traces",
        meta.len(),
        traces_data.len()
    );
    let name = |p: &std::path::Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    if let Some(other) = against {
        let base = jsonl(other)?;
        anyhow::ensure!(
            meta.len() == base.len(),
            "{} cases, {} traces",
            meta.len(),
            base.len()
        );
        let report = score::paired(&meta, &base, &traces_data);
        if json {
            println!("{}", pyjson::dumps_indent(&report, 2));
            return Ok(());
        }
        for (key, metrics) in report.as_object().into_iter().flatten() {
            println!("[{key}] {} against {}", name(traces), name(other));
            for (k, r) in metrics.as_object().into_iter().flatten() {
                if r["gained"] != 0 || r["lost"] != 0 {
                    println!(
                        "  {k:<14} +{} -{}  p={}",
                        r["gained"],
                        r["lost"],
                        score::pyrepr(&r["p"])
                    );
                }
            }
        }
        return Ok(());
    }
    let report = score::report(&meta, &traces_data);
    if json {
        println!("{}", pyjson::dumps_indent(&report, 2));
        return Ok(());
    }
    for (key, section) in report.as_object().into_iter().flatten() {
        let mut section = section.as_object().cloned().unwrap_or_default();
        let n = section.shift_remove("n").unwrap_or_default();
        println!("[{key}] n={n}");
        for (k, v) in section {
            println!("  {k:<14} {}", score::pyrepr(&v));
        }
    }
    Ok(())
}
