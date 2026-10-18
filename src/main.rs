use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use npu_commit::backend::{Backend, GenerationConfig, MockBackend};
use npu_commit::context::RepoContext;
use npu_commit::diff;
use npu_commit::genai::{Device, GenaiBackend, GenaiOptions};
use npu_commit::generate::{self, Source};
use npu_commit::git::Repo;
use npu_commit::hook;
use npu_commit::message::CommitMessage;
use npu_commit::paths;
use npu_commit::prompt::{self, PromptOptions};
use npu_commit::trace::Trace;

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
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Show the prompt the model would get for the staged changes.
    Prompt {
        #[arg(default_value = ".")]
        path: PathBuf,
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
    /// Manage the `prepare-commit-msg` git hook.
    #[command(subcommand)]
    Hook(HookCommand),
}

#[derive(Debug, Subcommand)]
enum HookCommand {
    /// Fill in the message of every plain `git commit`, from any client.
    Install {
        #[arg(short = 'C', long = "path", default_value = ".")]
        path: PathBuf,
        /// Replace a hook that was not installed by npu-commit; it is kept
        /// as a backup and restored by `uninstall`.
        #[arg(long)]
        force: bool,
    },
    /// Remove the hook, restoring the one it replaced, if any.
    Uninstall {
        #[arg(short = 'C', long = "path", default_value = ".")]
        path: PathBuf,
    },
    /// Called by git; never fails the commit.
    #[command(hide = true)]
    Run {
        /// The file holding the commit message.
        message_file: PathBuf,
        /// Where the message came from, if anywhere.
        source: Option<String>,
        /// The commit, for amends.
        sha: Option<String>,
        #[command(flatten)]
        backend: BackendArgs,
    },
}

#[derive(Debug, Args)]
struct GenerateArgs {
    /// Repository to work on.
    #[arg(short = 'C', long = "path", default_value = ".")]
    path: PathBuf,
    /// Open the editor with the message, then commit.
    #[arg(long)]
    commit: bool,
    #[command(flatten)]
    backend: BackendArgs,
}

/// How messages are generated. Each option can also come from the
/// environment, which is how the git hook, run by git without our flags,
/// gets them.
#[derive(Debug, Args)]
struct BackendArgs {
    #[arg(long, env = "NPU_COMMIT_BACKEND", value_enum, default_value_t = BackendKind::Openvino)]
    backend: BackendKind,
    #[command(flatten)]
    model: ModelArgs,
    /// Reply of the mock backend; repeat it for the following attempts.
    /// The environment variable holds a single reply.
    #[arg(
        long = "mock-response",
        env = "NPU_COMMIT_MOCK_RESPONSE",
        value_name = "TEXT"
    )]
    mock_responses: Vec<String>,
    /// Record inputs, prompts and replies to FILE, for `replay`. It
    /// includes the staged diff and recent commit subjects.
    #[arg(long, env = "NPU_COMMIT_TRACE", value_name = "FILE")]
    trace: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendKind {
    /// The exported model through OpenVINO GenAI, on --device.
    Openvino,
    /// Scripted replies from --mock-response, for tests and development.
    Mock,
}

/// Which model runs where, for the OpenVINO backend.
#[derive(Debug, Args)]
struct ModelArgs {
    #[arg(long, env = "NPU_COMMIT_DEVICE", value_enum, default_value_t = DeviceArg::Npu)]
    device: DeviceArg,
    /// Exported model [default: $XDG_DATA_HOME/npu-commit/models/<default model>]
    #[arg(long, env = "NPU_COMMIT_MODEL_DIR", value_name = "DIR")]
    model_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DeviceArg {
    Npu,
    Gpu,
    Cpu,
}

impl ModelArgs {
    fn load(&self) -> Result<GenaiBackend, Box<dyn Error>> {
        let env = |name: &str| std::env::var(name).ok();
        let model_dir = self
            .model_dir
            .clone()
            .or_else(|| paths::default_model_dir(env))
            .ok_or("HOME is not set; pass --model-dir")?;
        let cache_dir = paths::default_cache_dir(env).ok_or("HOME is not set")?;
        let device = match self.device {
            DeviceArg::Npu => Device::Npu,
            DeviceArg::Gpu => Device::Gpu,
            DeviceArg::Cpu => Device::Cpu,
        };
        Ok(GenaiBackend::load(&GenaiOptions {
            model_dir,
            device,
            cache_dir,
            // The NPU compiles for exactly the prompt length we budget for.
            max_prompt_tokens: PromptOptions::default().max_prompt_tokens,
            max_new_tokens: GenerationConfig::default().max_new_tokens,
        })?)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Context { path }) => run_context(&path),
        Some(Command::Prompt { path }) => run_prompt(&path),
        Some(Command::Replay {
            trace,
            with_model,
            model,
        }) => run_replay(&trace, with_model.then_some(&model)),
        Some(Command::Hook(HookCommand::Install { path, force })) => run_hook_install(&path, force),
        Some(Command::Hook(HookCommand::Uninstall { path })) => run_hook_uninstall(&path),
        Some(Command::Hook(HookCommand::Run {
            message_file,
            source,
            sha: _,
            backend,
        })) => {
            run_hook(&message_file, source.as_deref(), &backend);
            Ok(())
        }
        None => run_generate(&cli.generate),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_context(path: &Path) -> Result<(), Box<dyn Error>> {
    let context = RepoContext::collect(&Repo::discover(path)?)?;
    print_context(&context);
    Ok(())
}

/// Prints the prompt the model would get for the staged changes.
fn run_prompt(path: &Path) -> Result<(), Box<dyn Error>> {
    let repo = Repo::discover(path)?;
    let context = RepoContext::collect(&repo)?;
    let files = staged_files(&repo)?;
    let options = PromptOptions::default();
    let prompt = prompt::build(&context, &files, &options);
    let tokens = diff::estimate_tokens(&prompt.system) + diff::estimate_tokens(&prompt.user);
    println!("===== system =====\n{}", prompt.system);
    println!("===== user =====\n{}", prompt.user);
    println!(
        "===== ~{tokens} of {} tokens =====",
        options.max_prompt_tokens
    );
    Ok(())
}

fn run_generate(args: &GenerateArgs) -> Result<(), Box<dyn Error>> {
    let repo = Repo::discover(&args.path)?;
    let message = generate_message(&repo, &args.backend)?.to_string();
    if args.commit {
        repo.commit_in_editor(&message)?;
    } else {
        println!("{message}");
    }
    Ok(())
}

/// Generates a message for what is staged in `repo`, writing the trace if
/// asked and warning when the fallback had to be used.
fn generate_message(repo: &Repo, args: &BackendArgs) -> Result<CommitMessage, Box<dyn Error>> {
    let context = RepoContext::collect(repo)?;
    let files = staged_files(repo)?;
    let backend: Box<dyn Backend> = match args.backend {
        BackendKind::Openvino => Box::new(args.model.load()?),
        BackendKind::Mock => Box::new(MockBackend::new(args.mock_responses.clone())),
    };
    let options = PromptOptions::default();
    let config = GenerationConfig::default();
    let generation = generate::generate(&context, &files, backend.as_ref(), &options, &config)?;
    if let Some(path) = &args.trace {
        let trace = Trace::new(
            &context,
            &files,
            backend.as_ref(),
            &options,
            &config,
            &generation,
        );
        std::fs::write(path, trace.to_json())
            .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    }
    if generation.source == Source::Fallback {
        eprintln!(
            "warning: the model's output was unusable after {} attempts; \
             using a message built from the file list",
            generation.attempts.len()
        );
    }
    Ok(generation.message)
}

fn run_replay(path: &Path, model: Option<&ModelArgs>) -> Result<(), Box<dyn Error>> {
    let json = std::fs::read_to_string(path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let trace = Trace::from_json(&json)?;
    let replay = match model {
        None => trace.replay()?,
        Some(model) => {
            let backend = model.load()?;
            if backend.describe() != trace.backend {
                eprintln!(
                    "note: recorded with `{}`, replaying with `{}`",
                    trace.backend,
                    backend.describe()
                );
            }
            trace.replay_with(&backend)?
        }
    };
    if !replay.differences.is_empty() {
        for difference in &replay.differences {
            eprintln!("differs: {difference}");
        }
        return Err(format!(
            "not reproduced (trace written by npu-commit {}, this is {})",
            trace.tool_version,
            env!("CARGO_PKG_VERSION")
        )
        .into());
    }
    println!(
        "reproduced: {} attempt(s), {:?} message",
        replay.generation.attempts.len(),
        replay.generation.source
    );
    println!("{}", replay.generation.message);
    Ok(())
}

const HOOK_NAME: &str = "prepare-commit-msg";
const HOOK_BACKUP: &str = "prepare-commit-msg.npu-commit-backup";

fn run_hook_install(path: &Path, force: bool) -> Result<(), Box<dyn Error>> {
    let dir = Repo::discover(path)?.hooks_dir()?;
    let hook = dir.join(HOOK_NAME);
    if let Ok(existing) = std::fs::read_to_string(&hook)
        && !hook::is_ours(&existing)
    {
        if !force {
            return Err(format!(
                "{} already exists and was not installed by npu-commit; \
                 use --force to replace it (it will be kept as a backup)",
                hook.display()
            )
            .into());
        }
        let backup = dir.join(HOOK_BACKUP);
        if backup.exists() {
            return Err(format!(
                "{} already exists; move it away before replacing another hook",
                backup.display()
            )
            .into());
        }
        std::fs::rename(&hook, &backup)?;
        println!("backed up the existing hook to {}", backup.display());
    }
    let binary = std::env::current_exe()?.canonicalize()?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&hook, hook::script(&binary))?;
    make_executable(&hook)?;
    println!("installed {}", hook.display());
    Ok(())
}

fn run_hook_uninstall(path: &Path) -> Result<(), Box<dyn Error>> {
    let dir = Repo::discover(path)?.hooks_dir()?;
    let hook = dir.join(HOOK_NAME);
    let existing = std::fs::read_to_string(&hook)
        .map_err(|_| format!("no {HOOK_NAME} hook in {}", dir.display()))?;
    if !hook::is_ours(&existing) {
        return Err(format!(
            "{} was not installed by npu-commit; leaving it alone",
            hook.display()
        )
        .into());
    }
    std::fs::remove_file(&hook)?;
    println!("removed {}", hook.display());
    let backup = dir.join(HOOK_BACKUP);
    if backup.exists() {
        std::fs::rename(&backup, &hook)?;
        println!("restored the previous hook");
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Fills in the message file, or warns and leaves it as is: a hook that
/// fails would block the commit.
fn run_hook(message_file: &Path, source: Option<&str>, backend: &BackendArgs) {
    if !hook::should_fill(source) {
        return;
    }
    if let Err(err) = fill_message_file(message_file, backend) {
        eprintln!("npu-commit: {err}; the commit message was left as is");
    }
}

fn fill_message_file(message_file: &Path, backend: &BackendArgs) -> Result<(), Box<dyn Error>> {
    // Git runs hooks from the root of the work tree, with GIT_INDEX_FILE
    // pointing at the index being committed (e.g. for `commit -a`).
    let repo = Repo::discover(".")?;
    let message = generate_message(&repo, backend)?.to_string();
    let existing = std::fs::read_to_string(message_file)?;
    if let Some(filled) = hook::prefill(&existing, &message, &repo.comment_prefix()?) {
        std::fs::write(message_file, filled)?;
    }
    Ok(())
}

fn staged_files(repo: &Repo) -> Result<Vec<diff::FileDiff>, Box<dyn Error>> {
    let files = repo.staged_diff()?;
    if files.is_empty() {
        return Err("nothing staged".into());
    }
    Ok(files)
}

fn print_context(ctx: &RepoContext) {
    let style = &ctx.style;
    println!("repository:  {}", ctx.root.display());
    println!(
        "branch:      {}",
        ctx.branch.as_deref().unwrap_or("(detached)")
    );
    println!("ticket:      {}", ctx.ticket.as_deref().unwrap_or("-"));
    println!(
        "convention:  {:?} (from {} commits)",
        style.convention, style.sample_size
    );
    println!("language:    {:?}", style.language);
    println!("tickets:     {:?}", style.tickets);
    println!("types:       {}", format_ranked(&style.types));
    println!("scopes:      {}", format_ranked(&style.scopes));
    println!("examples:");
    for example in &style.examples {
        println!("  {example}");
    }
    println!("staged ({}):", ctx.staged.len());
    for change in &ctx.staged {
        match &change.old_path {
            Some(old) => println!("  {:?} {old} -> {}", change.status, change.path),
            None => println!("  {:?} {}", change.status, change.path),
        }
    }
}

fn format_ranked(items: &[(String, usize)]) -> String {
    if items.is_empty() {
        return "-".to_owned();
    }
    items
        .iter()
        .map(|(name, count)| format!("{name} ({count})"))
        .collect::<Vec<_>>()
        .join(", ")
}
