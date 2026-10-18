use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use smart_commit::backend::{Backend, BackendError, MockBackend};
use smart_commit::config::{self, BackendKind, DeviceSetting, Resolved, Settings, Sources};
use smart_commit::context::RepoContext;
use smart_commit::device::{self, Target};
use smart_commit::diff;
use smart_commit::genai::{Device, GenaiBackend};
use smart_commit::generate::{self, Source};
use smart_commit::git::Repo;
use smart_commit::hook;
use smart_commit::llamacpp::LlamaCppBackend;
use smart_commit::message::CommitMessage;
use smart_commit::paths;
use smart_commit::prompt;
use smart_commit::trace::Trace;

mod bench_cmd;
mod config_cmd;

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
        #[arg(short = 'C', long = "path", default_value = ".")]
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
    /// Read and change settings, globally or for a repository.
    #[command(subcommand)]
    Config(config_cmd::ConfigCommand),
    /// Measure every device of this machine; `auto` then uses the fastest.
    Bench {
        /// Print the full report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show which device runs the model and how fast each one was.
    Device {
        #[command(subcommand)]
        command: Option<bench_cmd::DeviceCommand>,
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
}

#[derive(Debug, Subcommand)]
enum HookCommand {
    /// Fill in the message of every plain `git commit`, from any client.
    Install {
        #[arg(short = 'C', long = "path", default_value = ".")]
        path: PathBuf,
        /// Replace a hook that was not installed by smart-commit; it is kept
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

/// How messages are generated. Unset options come from the environment,
/// then the config files (see `smart-commit config list --show-origin`).
#[derive(Debug, Args)]
struct BackendArgs {
    /// [config: backend; env: SMART_COMMIT_BACKEND]
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    #[command(flatten)]
    model: ModelArgs,
    /// Reply of the mock backend; repeat it for the following attempts.
    /// The environment variable holds a single reply.
    #[arg(
        long = "mock-response",
        env = "SMART_COMMIT_MOCK_RESPONSE",
        value_name = "TEXT"
    )]
    mock_responses: Vec<String>,
    /// Record inputs, prompts and replies to FILE, for `replay`. It
    /// includes the staged diff and recent commit subjects.
    #[arg(long, env = "SMART_COMMIT_TRACE", value_name = "FILE")]
    trace: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    /// Whichever --device needs.
    Auto,
    /// OpenVINO GenAI: Intel NPU, Intel GPU or CPU.
    Openvino,
    /// llama.cpp with Vulkan: any GPU (AMD, NVIDIA, Intel).
    Llamacpp,
    /// Scripted replies from --mock-response, for tests and development.
    Mock,
}

/// Which model runs where, for the OpenVINO backend.
#[derive(Debug, Args)]
struct ModelArgs {
    /// [config: model.device; env: SMART_COMMIT_DEVICE]
    #[arg(long, value_enum)]
    device: Option<DeviceArg>,
    /// [config: model.dir; env: SMART_COMMIT_MODEL_DIR]
    #[arg(long, value_name = "DIR")]
    model_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeviceArg {
    /// The fastest benchmarked device, else the first that works.
    Auto,
    /// Intel NPU, through OpenVINO.
    Npu,
    /// Intel GPU, through OpenVINO.
    #[value(alias = "gpu")]
    IntelGpu,
    /// Any GPU with a Vulkan driver (AMD, NVIDIA, Intel), through llama.cpp.
    Vulkan,
    /// Any x86_64 CPU (Intel or AMD), through llama.cpp.
    Cpu,
    /// The CPU through OpenVINO, when it is installed for an Intel NPU or GPU.
    OpenvinoCpu,
}

impl DeviceArg {
    /// The device it names; none for `auto`.
    pub fn target(self) -> Option<Target> {
        match self {
            Self::Auto => None,
            Self::Npu => Some(Target::Npu),
            Self::IntelGpu => Some(Target::IntelGpu),
            Self::Vulkan => Some(Target::Vulkan),
            Self::Cpu => Some(Target::Cpu),
            Self::OpenvinoCpu => Some(Target::OpenvinoCpu),
        }
    }

    /// The value `model.device` takes for it.
    pub fn name(self) -> &'static str {
        self.target().map_or("auto", Target::name)
    }
}

impl BackendArgs {
    fn flags(&self) -> Result<Vec<(&'static str, String)>, Box<dyn Error>> {
        let mut flags = self.model.flags()?;
        if let Some(backend) = self.backend {
            let name = match backend {
                BackendArg::Auto => "auto",
                BackendArg::Openvino => "openvino",
                BackendArg::Llamacpp => "llamacpp",
                BackendArg::Mock => "mock",
            };
            flags.push(("backend", name.to_owned()));
        }
        Ok(flags)
    }
}

impl ModelArgs {
    fn flags(&self) -> Result<Vec<(&'static str, String)>, Box<dyn Error>> {
        let mut flags = Vec::new();
        if let Some(device) = self.device {
            flags.push(("model.device", device.name().to_owned()));
        }
        if let Some(dir) = &self.model_dir {
            // Relative to where the command runs, as users expect from a flag.
            let dir = std::path::absolute(dir)?;
            flags.push(("model.dir", dir.to_string_lossy().into_owned()));
        }
        Ok(flags)
    }
}

/// Every layer for `repo` (or none outside a repository), with `flags` on
/// top.
fn resolve(
    repo: Option<&Repo>,
    flags: Vec<(&'static str, String)>,
) -> Result<Resolved, config::ConfigError> {
    let env = |name: &str| std::env::var(name).ok();
    Resolved::resolve(&Sources {
        global: paths::default_config_file(env),
        repo: repo.map(|r| r.root().join(config::REPO_FILE)),
        env: &env,
        flags,
    })
}

/// Warns about settings that still use an old name.
pub fn warn_renamed(resolved: &Resolved) {
    for warning in resolved.renamed_values() {
        eprintln!("warning: {warning}");
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
        Some(Command::Config(command)) => config_cmd::run(command),
        Some(Command::Bench { json }) => bench_cmd::run_bench(json),
        Some(Command::Device { command }) => bench_cmd::run_device(command),
        Some(Command::Setup {
            yes,
            no_drivers,
            no_bench,
        }) => run_setup(yes, no_drivers, no_bench),
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
    let repo = Repo::discover(path)?;
    let settings = resolve(Some(&repo), Vec::new())?.settings();
    let context = RepoContext::collect_with_history(&repo, settings.history_depth)?;
    print_context(&context, &settings);
    Ok(())
}

/// Prints the prompt the model would get for the staged changes.
fn run_prompt(path: &Path) -> Result<(), Box<dyn Error>> {
    let repo = Repo::discover(path)?;
    let settings = resolve(Some(&repo), Vec::new())?.settings();
    let context = RepoContext::collect_with_history(&repo, settings.history_depth)?;
    let files = staged_files(&repo)?;
    let options = settings.prompt_options();
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

/// Loads the model on the configured device, or picks one for `auto`.
fn load_model(settings: &Settings) -> Result<Box<dyn Backend>, Box<dyn Error>> {
    let candidates = device::candidates(settings.backend, settings.device, &preferred_devices())?;
    let selection = device::load_first(&candidates, |target| load_on(settings, target))?;
    if settings.device == DeviceSetting::Auto && !selection.skipped.is_empty() {
        let skipped: Vec<&str> = selection.skipped.iter().map(|(t, _)| t.name()).collect();
        eprintln!(
            "note: running on {}; {} unavailable (`smart-commit device` shows why)",
            selection.target,
            skipped.join(", ")
        );
    }
    Ok(selection.backend)
}

/// Loads on `target`; on llama.cpp devices, with fast load as
/// `model.fast_load` and the last benchmark say.
fn load_on(settings: &Settings, target: Target) -> Result<Box<dyn Backend>, BackendError> {
    let fast_load = settings
        .fast_load
        .enabled(|| bench_cmd::last_report().is_some_and(|report| report.fast_load(target)));
    load_with(settings, target, fast_load, Retry::Normally)
}

/// What [`load_with`] does when fast load fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// Loads normally, with a note: what a commit wants.
    Normally,
    /// Fails: what the benchmark, which measures each mode, wants.
    Never,
}

/// Loads on `target`, with or without fast load on llama.cpp devices.
fn load_with(
    settings: &Settings,
    target: Target,
    fast_load: bool,
    retry: Retry,
) -> Result<Box<dyn Backend>, BackendError> {
    let device = match target {
        Target::Vulkan | Target::Cpu => {
            let options = settings
                .llamacpp_options(target, fast_load)
                .map_err(BackendError::Unavailable)?;
            if retry == Retry::Never {
                return Ok(Box::new(LlamaCppBackend::start(&options)?));
            }
            let (backend, note) = LlamaCppBackend::start_or_load_normally(&options)?;
            if let Some(note) = note {
                eprintln!("note: {note}");
            }
            return Ok(Box::new(backend));
        }
        Target::Npu => Device::Npu,
        Target::IntelGpu => Device::Gpu,
        Target::OpenvinoCpu => Device::Cpu,
    };
    let options = settings
        .genai_options(device)
        .map_err(BackendError::Unavailable)?;
    Ok(Box::new(GenaiBackend::load(&options)?))
}

/// Devices in the order the last benchmark ranked them, fastest first.
fn preferred_devices() -> Vec<Target> {
    bench_cmd::last_report()
        .map(|report| report.ranking())
        .unwrap_or_default()
}

/// Generates a message for what is staged in `repo`, writing the trace if
/// asked and warning when the fallback had to be used.
fn generate_message(repo: &Repo, args: &BackendArgs) -> Result<CommitMessage, Box<dyn Error>> {
    let resolved = resolve(Some(repo), args.flags()?)?;
    warn_renamed(&resolved);
    let settings = resolved.settings();
    let context = RepoContext::collect_with_history(repo, settings.history_depth)?;
    let files = staged_files(repo)?;
    let backend: Box<dyn Backend> = match settings.backend {
        BackendKind::Mock => Box::new(MockBackend::new(args.mock_responses.clone())),
        _ => load_model(&settings)?,
    };
    let options = settings.prompt_options();
    let config = settings.generation_config();
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
            // The prompt options come from the trace; only where the model
            // runs comes from the settings (no repository: it may be gone).
            let settings = resolve(None, model.flags()?)?.settings();
            let backend = load_model(&settings)?;
            if backend.describe() != trace.backend {
                eprintln!(
                    "note: recorded with `{}`, replaying with `{}`",
                    trace.backend,
                    backend.describe()
                );
            }
            trace.replay_with(backend.as_ref())?
        }
    };
    if !replay.differences.is_empty() {
        for difference in &replay.differences {
            eprintln!("differs: {difference}");
        }
        return Err(format!(
            "not reproduced (trace written by smart-commit {}, this is {})",
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

/// Runs `setup.sh`: installed next to the binary (`../share/smart-commit`)
/// or, from a checkout, in `scripts/`.
fn run_setup(yes: bool, no_drivers: bool, no_bench: bool) -> Result<(), Box<dyn Error>> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let bin = exe.parent().ok_or("the binary has no directory")?;
    let script = [
        bin.join("../share/smart-commit/scripts/setup.sh"),
        bin.join("../../scripts/setup.sh"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .ok_or("setup.sh not found next to the binary; reinstall smart-commit")?;
    let flags = [
        (yes, "--yes"),
        (no_drivers, "--no-drivers"),
        (no_bench, "--no-bench"),
    ];
    let status = std::process::Command::new(&script)
        .args(flags.iter().filter(|(on, _)| *on).map(|(_, flag)| flag))
        // So the setup benchmarks this very binary.
        .env("SMART_COMMIT_BIN", &exe)
        .status()
        .map_err(|err| format!("cannot run {}: {err}", script.display()))?;
    if !status.success() {
        return Err(format!("setup failed ({status})").into());
    }
    Ok(())
}

const HOOK_NAME: &str = "prepare-commit-msg";
const HOOK_BACKUP: &str = "prepare-commit-msg.smart-commit-backup";

fn run_hook_install(path: &Path, force: bool) -> Result<(), Box<dyn Error>> {
    let dir = Repo::discover(path)?.hooks_dir()?;
    let hook = dir.join(HOOK_NAME);
    if let Ok(existing) = std::fs::read_to_string(&hook)
        && !hook::is_ours(&existing)
    {
        if !force {
            return Err(format!(
                "{} already exists and was not installed by smart-commit; \
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
            "{} was not installed by smart-commit; leaving it alone",
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
        eprintln!("smart-commit: {err}; the commit message was left as is");
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

fn print_context(ctx: &RepoContext, settings: &Settings) {
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
    if let Some(forced) = settings.convention {
        println!("             forced to {forced:?} by style.convention");
    }
    println!("language:    {:?}", style.language);
    if let Some(forced) = settings.language {
        println!("             forced to {forced:?} by style.language");
    }
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
