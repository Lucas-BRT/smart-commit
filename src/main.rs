use std::error::Error;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Parser, Subcommand, ValueEnum};
use smart_commit::backend::{Backend, BackendError, GenerationConfig, MockBackend};
use smart_commit::config::{
    self, BackendKind, DeviceSetting, Origin, Resolved, Settings, Sources, Value,
};
use smart_commit::context::RepoContext;
use smart_commit::device::{self, Target};
use smart_commit::diff::{self, FileDiff};
use smart_commit::genai::{Device, GenaiBackend};
use smart_commit::generate::{self, Generation, Source};
use smart_commit::git::Repo;
use smart_commit::hardware::{self, Hardware};
use smart_commit::hook;
use smart_commit::llamacpp::LlamaCppBackend;
use smart_commit::message::CommitMessage;
use smart_commit::paths;
use smart_commit::prompt::{self, PromptOptions};
use smart_commit::trace::{Inputs, Trace};
use smart_commit::tui::review::{self, Review};
use smart_commit::tui::{self, Screen};

mod bench_cmd;
mod config_cmd;
mod model_cmd;
mod setup_cmd;

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
    /// The CPU, GPUs and NPU of this machine, and the devices each gives.
    Hardware,
    /// List the recommended models, or switch to another one or to a file.
    Model {
        #[command(subcommand)]
        command: Option<model_cmd::ModelCommand>,
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
    /// Review the message (commit, edit, ask for another), then commit; outside
    /// a terminal, open the editor with it.
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
        Some(Command::Prompt {
            inputs: Some(inputs),
            ..
        }) => run_prompt_inputs(&inputs),
        Some(Command::Prompt { path, inputs: None }) => run_prompt(&path),
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
        Some(Command::Model { command }) => model_cmd::run(command),
        Some(Command::Hardware) => {
            print!("{}", Hardware::detect(&hardware_sources()).inventory());
            Ok(())
        }
        Some(Command::Setup {
            yes,
            no_drivers,
            no_bench,
        }) => setup_cmd::run(yes, no_drivers, no_bench),
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

/// `prompt --inputs`: the prompts of many recorded inputs, e.g. to build a
/// training set with the exact prompts smart-commit sends. Only the user's
/// global settings apply, since there is no repository.
fn run_prompt_inputs(path: &Path) -> Result<(), Box<dyn Error>> {
    let text = if path == Path::new("-") {
        std::io::read_to_string(std::io::stdin())?
    } else {
        std::fs::read_to_string(path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?
    };
    let options = resolve(None, Vec::new())?.settings().prompt_options();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let inputs: Inputs = serde_json::from_str(line)
            .map_err(|err| format!("{}:{}: invalid inputs: {err}", path.display(), index + 1))?;
        let prompt = prompt::build(&inputs.context(), &inputs.files, &options);
        writeln!(out, "{}", serde_json::to_string(&prompt)?)?;
    }
    out.flush()?;
    Ok(())
}

fn run_generate(args: &GenerateArgs) -> Result<(), Box<dyn Error>> {
    let repo = Repo::discover(&args.path)?;
    if args.commit && tui::interactive() {
        return review_and_commit(&repo, &args.backend);
    }
    let message = generate_message(&repo, &args.backend)?.to_string();
    if args.commit {
        repo.commit_in_editor(&message)?;
    } else {
        println!("{message}");
    }
    Ok(())
}

/// `--commit` in a terminal: shows the message, then commits it, opens the
/// editor, asks the model for another one or gives up.
fn review_and_commit(repo: &Repo, args: &BackendArgs) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let session = Session::start(repo, args)?;
    let generation = session.generate(&[])?;
    let mut review = Review::new(
        &generation,
        session.running.clone(),
        started.elapsed().as_secs_f32(),
        &session.context.staged,
    );
    let mut screen = Screen::new()?;
    let action = loop {
        screen.draw(|frame| review.render(frame))?;
        match review.on_key(screen.key()?) {
            review::Action::None => {}
            review::Action::Another => {
                review.working();
                screen.draw(|frame| review.render(frame))?;
                let started = Instant::now();
                let generation = session.generate(&review.turned_down)?;
                review.replace(&generation, started.elapsed().as_secs_f32());
            }
            action => break action,
        }
    };
    drop(screen);
    let message = review.message.to_string();
    match action {
        review::Action::Commit => repo.commit(&message)?,
        review::Action::Edit => repo.commit_in_editor(&message)?,
        _ => eprintln!("Cancelled: nothing was committed."),
    }
    Ok(())
}

/// A loaded model and what a message is made from, kept to ask again.
struct Session {
    context: RepoContext,
    files: Vec<FileDiff>,
    backend: Box<dyn Backend>,
    options: PromptOptions,
    config: GenerationConfig,
    /// The model and the device, for people: `qwen2.5-coder-1.5b on cpu`.
    running: String,
    trace: Option<PathBuf>,
}

impl Session {
    fn start(repo: &Repo, args: &BackendArgs) -> Result<Self, Box<dyn Error>> {
        let resolved = resolve(Some(repo), args.flags()?)?;
        warn_renamed(&resolved);
        let settings = resolved.settings();
        let context = RepoContext::collect_with_history(repo, settings.history_depth)?;
        let files = staged_files(repo)?;
        let (backend, running): (Box<dyn Backend>, String) = match settings.backend {
            BackendKind::Mock => (
                Box::new(MockBackend::new(args.mock_responses.clone())),
                "the mock backend".to_owned(),
            ),
            _ => {
                let (backend, target) = load_model(&settings)?;
                (
                    backend,
                    format!("{} on {target}", model_label(&resolved, target)),
                )
            }
        };
        Ok(Self {
            context,
            files,
            backend,
            options: settings.prompt_options(),
            config: settings.generation_config(),
            running,
            trace: args.trace.clone(),
        })
    }

    /// A message other than those in `avoid`, writing the trace if asked.
    fn generate(&self, avoid: &[String]) -> Result<Generation, Box<dyn Error>> {
        let generation = generate::generate_avoiding(
            &self.context,
            &self.files,
            self.backend.as_ref(),
            &self.options,
            &self.config,
            avoid,
        )?;
        if let Some(path) = &self.trace {
            let trace = Trace::new(
                &self.context,
                &self.files,
                self.backend.as_ref(),
                &self.options,
                &self.config,
                &generation,
            );
            std::fs::write(path, trace.to_json())
                .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
        }
        Ok(generation)
    }
}

/// The model's id, or the file's name when model.gguf or model.dir (for
/// `target`) points elsewhere.
fn model_label(resolved: &Resolved, target: Target) -> String {
    let key = match target.backend() {
        BackendKind::LlamaCpp => "model.gguf",
        _ => "model.dir",
    };
    match resolved.get(key) {
        Some((Value::Text(path), origin)) if *origin != Origin::Default => Path::new(path)
            .file_name()
            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned()),
        _ => resolved.settings().model.id.to_owned(),
    }
}

/// Loads the model on the configured device, or picks one for `auto`.
fn load_model(settings: &Settings) -> Result<(Box<dyn Backend>, Target), Box<dyn Error>> {
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
    Ok((selection.backend, selection.target))
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
    let generation = Session::start(repo, args)?.generate(&[])?;
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
            let (backend, _) = load_model(&settings)?;
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
/// The real machine, or the fake one tests point at (the variables
/// `scripts/hardware.sh` reads too).
fn hardware_sources() -> hardware::Sources {
    let real = hardware::Sources::real();
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    hardware::Sources {
        sysfs: var("SMART_COMMIT_SYSFS").unwrap_or(real.sysfs),
        cpuinfo: var("SMART_COMMIT_CPUINFO").unwrap_or(real.cpuinfo),
        openvino_dir: var("SMART_COMMIT_OPENVINO_DIR").unwrap_or(real.openvino_dir),
    }
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
