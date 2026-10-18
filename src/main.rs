use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use npu_commit::backend::{Backend, GenerationConfig, MockBackend};
use npu_commit::context::RepoContext;
use npu_commit::diff;
use npu_commit::generate::{self, Source};
use npu_commit::git::Repo;
use npu_commit::prompt::{self, PromptOptions};

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
}

#[derive(Debug, Args)]
struct GenerateArgs {
    /// Repository to work on.
    #[arg(short = 'C', long = "path", default_value = ".")]
    path: PathBuf,
    /// Open the editor with the message, then commit.
    #[arg(long)]
    commit: bool,
    #[arg(long, value_enum, default_value_t = BackendKind::Npu)]
    backend: BackendKind,
    /// Reply of the mock backend; repeat it for the following attempts.
    #[arg(long = "mock-response", value_name = "TEXT")]
    mock_responses: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendKind {
    /// Intel NPU through OpenVINO (not available yet).
    Npu,
    /// Scripted replies from --mock-response, for tests and development.
    Mock,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Context { path }) => run_context(&path),
        Some(Command::Prompt { path }) => run_prompt(&path),
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
    let context = RepoContext::collect(&repo)?;
    let files = staged_files(&repo)?;
    let backend: Box<dyn Backend> = match args.backend {
        BackendKind::Npu => {
            return Err("the NPU backend is not available yet; use --backend mock".into());
        }
        BackendKind::Mock => Box::new(MockBackend::new(args.mock_responses.clone())),
    };
    let generation = generate::generate(
        &context,
        &files,
        backend.as_ref(),
        &PromptOptions::default(),
        &GenerationConfig::default(),
    )?;
    if generation.source == Source::Fallback {
        eprintln!(
            "warning: the model's output was unusable after {} attempts; \
             using a message built from the file list",
            generation.attempts.len()
        );
    }
    let message = generation.message.to_string();
    if args.commit {
        repo.commit_in_editor(&message)?;
    } else {
        println!("{message}");
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
