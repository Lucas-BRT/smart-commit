//! The command line: the arguments of every command, and one module for
//! each of them.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod args;
mod benchmark;
mod configure;
mod git_hook;
mod guided_setup;
mod inspect;
mod install;
mod keep_loaded;
mod load;
mod model;
mod session;
mod settings;
mod suggest;

use args::{BackendArgs, GenerateArgs, ModelArgs, PromptArgs};
use git_hook::HookCommand;
use keep_loaded::ServerCommand;

/// Writes a commit message for the staged changes, in the style of the
/// repository, with a small LLM running locally.
#[derive(Debug, Parser)]
#[command(version, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    generate: GenerateArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show what was detected about the repository.
    Context {
        /// Repository to work on.
        #[arg(short = 'C', long = "path", default_value = ".")]
        path: PathBuf,
    },
    /// Show the prompt the model would get for the staged changes.
    Prompt {
        /// Repository to work on.
        #[arg(
            short = 'C',
            long = "path",
            default_value = ".",
            conflicts_with = "inputs"
        )]
        path: PathBuf,
        /// Build prompts without a repository from JSON Lines of trace
        /// inputs (`-` for stdin), printing one `{"system", "user"}` per line.
        #[arg(long, value_name = "FILE")]
        inputs: Option<PathBuf>,
        #[command(flatten)]
        prompt: PromptArgs,
    },
    /// Run a recorded generation again and check it is reproduced exactly.
    Replay {
        trace: PathBuf,
        /// Ask the model again instead of reusing the recorded replies, to
        /// check that it still answers the same way.
        #[arg(long)]
        with_model: bool,
        #[command(flatten)]
        model: ModelArgs,
    },
    /// Generate a message for each of many recorded inputs, without a
    /// repository, printing one compact trace per line.
    Eval {
        /// JSON Lines of trace inputs, as `prompt --inputs` reads (`-` for
        /// stdin).
        inputs: PathBuf,
        #[command(flatten)]
        backend: BackendArgs,
        #[command(flatten)]
        prompt: PromptArgs,
    },
    /// Manage the `prepare-commit-msg` git hook.
    #[command(subcommand)]
    Hook(HookCommand),
    /// Read and change settings, globally or for a repository.
    #[command(subcommand)]
    Config(configure::ConfigCommand),
    /// Measure every device of this machine; `auto` then uses the fastest.
    Bench {
        /// Print the full report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show which device runs the model and how fast each one was.
    Device {
        #[command(subcommand)]
        command: Option<benchmark::DeviceCommand>,
    },
    /// The CPU, GPUs and NPU of this machine, and the devices each gives.
    Hardware,
    /// List the recommended models, or switch to another one or to a file.
    Model {
        #[command(subcommand)]
        command: Option<model::ModelCommand>,
    },
    /// List this machine's CPU, GPUs and NPU, install only what they need,
    /// then benchmark them (asks before anything that needs sudo).
    Setup {
        /// Answer yes to every question.
        #[arg(long)]
        yes: bool,
        /// Skip drivers and runtimes: no sudo at all.
        #[arg(long)]
        no_drivers: bool,
        /// Skip the benchmark.
        #[arg(long)]
        no_bench: bool,
    },
    /// The models kept loaded between commits (`model.keep_loaded`).
    #[command(subcommand)]
    Server(ServerCommand),
    /// Install the latest release, if it is newer than this one (checked
    /// against its SHA256SUMS; models, settings and the benchmark are kept).
    Update,
    /// Remove smart-commit from this machine: the binary, the models,
    /// llama.cpp, the benchmark, the caches and the configuration.
    Uninstall {
        /// Remove without asking.
        #[arg(long)]
        yes: bool,
        /// Also remove OpenVINO, if smart-commit's setup installed it (uses
        /// sudo). Driver packages are kept.
        #[arg(long)]
        system: bool,
    },
    /// Runs a model kept loaded until it is idle; started by smart-commit.
    #[command(name = "keep-server", hide = true)]
    KeepServer { dir: PathBuf },
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Context { path }) => inspect::context(&path),
        Some(Command::Prompt {
            inputs: Some(inputs),
            prompt,
            ..
        }) => inspect::prompt_inputs(&inputs, &prompt),
        Some(Command::Prompt {
            path,
            inputs: None,
            prompt,
        }) => inspect::prompt(&path, &prompt),
        Some(Command::Replay {
            trace,
            with_model,
            model,
        }) => inspect::replay(&trace, with_model.then_some(&model)),
        Some(Command::Eval {
            inputs,
            backend,
            prompt,
        }) => inspect::eval(&inputs, &backend, &prompt),
        Some(Command::Hook(command)) => git_hook::run(command),
        Some(Command::Config(command)) => configure::run(command),
        Some(Command::Bench { json }) => benchmark::run_bench(json),
        Some(Command::Device { command }) => benchmark::run_device(command),
        Some(Command::Model { command }) => model::run(command),
        Some(Command::Hardware) => {
            inspect::hardware();
            Ok(())
        }
        Some(Command::Setup {
            yes,
            no_drivers,
            no_bench,
        }) => guided_setup::run(yes, no_drivers, no_bench),
        Some(Command::Server(command)) => keep_loaded::run(command),
        Some(Command::Update) => install::update(),
        Some(Command::Uninstall { yes, system }) => install::uninstall(yes, system),
        Some(Command::KeepServer { dir }) => smart_commit::server::serve(&dir).map_err(Into::into),
        None => suggest::run(&cli.generate),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
