//! The data tools for training smart-commit's models. They are a crate of
//! their own so that what they need (parquet, above all) never reaches the
//! binary users install.

mod build_inputs;
mod pyjson;
mod pytext;

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
    }
}
