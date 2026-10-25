//! Tools for working on smart-commit. They are a crate of their own so that
//! what they need never reaches the binary users install.

mod release_notes;
mod time_server;

use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use smart_commit::backend::device::Target;
use smart_commit::config::{DeviceSetting, Resolved, Sources};
use smart_commit::paths;

#[derive(Debug, Parser)]
#[command(name = "sc-tools", about = "Tools for working on smart-commit")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// The notes of release TAG: the commits since the previous `v*` tag,
    /// grouped by type, plus how to install (`REV` defaults to HEAD).
    ReleaseNotes {
        tag: String,
        #[arg(default_value = "HEAD")]
        rev: String,
    },
    /// Time llama-server on the `smart-commit prompt` outputs in DIR
    /// (`*.txt`), with the llama-server flags after `--`, to see what a flag
    /// costs. The model, server and budgets come from smart-commit's settings.
    TimeServer {
        dir: PathBuf,
        /// `vulkan` or `cpu` [default: model.device].
        #[arg(long)]
        device: Option<String>,
        /// The name printed on the result line.
        #[arg(long, default_value = "llama-server")]
        label: String,
        /// More llama-server flags, after smart-commit's own.
        #[arg(last = true)]
        server_args: Vec<String>,
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
        Command::ReleaseNotes { tag, rev } => {
            print!(
                "{}",
                release_notes::notes(std::path::Path::new("."), &tag, &rev)?
            );
            Ok(())
        }
        Command::TimeServer {
            dir,
            device,
            label,
            server_args,
        } => time_server(&dir, device, &label, &server_args),
    }
}

fn time_server(
    dir: &std::path::Path,
    device: Option<String>,
    label: &str,
    server_args: &[String],
) -> Result<()> {
    let env = |name: &str| std::env::var(name).ok();
    let resolved = Resolved::resolve(&Sources {
        global: paths::default_config_file(env),
        repo: None,
        env: &env,
        flags: device.map(|d| ("model.device", d)).into_iter().collect(),
    })?;
    let settings = resolved.settings();
    let target = match settings.device {
        DeviceSetting::Fixed(target @ (Target::Vulkan | Target::Cpu)) => target,
        _ => bail!("time-server times llama-server: choose --device vulkan or cpu"),
    };
    let options = settings
        .llamacpp_options(target, false)
        .map_err(|e| anyhow!(e))?;
    let prompts = time_server::read_prompts(dir).map_err(|e| anyhow!(e))?;
    let timing = time_server::time_server(
        &options,
        server_args,
        &prompts,
        &settings.generation_config(),
    )?;
    print!("{}", timing.render(label));
    Ok(())
}
