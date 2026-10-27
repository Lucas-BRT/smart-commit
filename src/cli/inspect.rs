//! The commands that show what smart-commit sees or would do: `context`,
//! `prompt`, `replay`, `eval` and `hardware`.

use std::io::Write as _;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use smart_commit::config::Settings;
use smart_commit::context::RepoContext;
use smart_commit::git::Repo;
use smart_commit::hardware::Hardware;
use smart_commit::trace::{Inputs, Trace};
use smart_commit::{diff, generate, prompt};

use super::args::{BackendArgs, ModelArgs, PromptArgs};
use super::load::load_model;
use super::session::{staged_files, start_backend};
use super::settings::{hardware_sources, resolve, warn_renamed};
use super::ui::note;

pub(super) fn context(path: &Path) -> Result<()> {
    let repo = Repo::discover(path)?;
    let settings = resolve(Some(&repo), Vec::new())?.settings();
    let context = RepoContext::collect_with_history(&repo, settings.history_depth)?;
    print_context(&context, &settings);
    Ok(())
}

/// Prints the prompt the model would get for the staged changes.
pub(super) fn prompt(path: &Path, prompt_args: &PromptArgs) -> Result<()> {
    let repo = Repo::discover(path)?;
    let settings = resolve(Some(&repo), Vec::new())?.settings();
    let context = RepoContext::collect_with_history(&repo, settings.history_depth)?;
    let files = staged_files(&repo, &context)?;
    let mut options = settings.prompt_options();
    prompt_args.apply(&mut options);
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
pub(super) fn prompt_inputs(path: &Path, prompt_args: &PromptArgs) -> Result<()> {
    let inputs = read_inputs(path)?;
    let mut options = resolve(None, Vec::new())?.settings().prompt_options();
    prompt_args.apply(&mut options);
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for case in &inputs {
        let prompt = prompt::build(&case.context(), &case.files, &options);
        writeln!(out, "{}", serde_json::to_string(&prompt)?)?;
    }
    out.flush()?;
    Ok(())
}

/// Reads JSON Lines of trace inputs from `path`, or stdin for `-`.
fn read_inputs(path: &Path) -> Result<Vec<Inputs>> {
    let text = if path == Path::new("-") {
        std::io::read_to_string(std::io::stdin())?
    } else {
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?
    };
    let mut inputs = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        inputs.push(
            serde_json::from_str(line)
                .with_context(|| format!("{}:{}: invalid inputs", path.display(), index + 1))?,
        );
    }
    Ok(inputs)
}

/// `eval`: the whole pipeline (prompt, model, sanitizer, retry, fallback)
/// on each recorded input, loading the model once. Each trace replays like
/// those of `--trace`. Like `prompt --inputs`, only global settings apply.
pub(super) fn eval(path: &Path, args: &BackendArgs, prompt_args: &PromptArgs) -> Result<()> {
    if args.trace.is_some() {
        bail!("eval writes its traces to stdout; --trace does not apply");
    }
    let inputs = read_inputs(path)?;
    let resolved = resolve(None, args.flags()?)?;
    warn_renamed(&resolved);
    let settings = resolved.settings();
    let (mut options, config) = (settings.prompt_options(), settings.generation_config());
    prompt_args.apply(&mut options);
    let (backend, running) = start_backend(&resolved, &args.mock_responses, false)?;
    eprintln!("evaluating {} input(s) with {running}", inputs.len());
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let options = generate::budgeted(&options, backend.as_ref());
    for case in &inputs {
        let ctx = case.context();
        let generation =
            generate::generate(&ctx, &case.files, backend.as_ref(), &options, &config)?;
        let trace = Trace::new(
            &ctx,
            &case.files,
            backend.as_ref(),
            &options,
            &config,
            &generation,
        );
        writeln!(out, "{}", serde_json::to_string(&trace)?)?;
    }
    out.flush()?;
    Ok(())
}

pub(super) fn replay(path: &Path, model: Option<&ModelArgs>) -> Result<()> {
    let json =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let trace = Trace::from_json(&json)?;
    let replay = match model {
        None => trace.replay()?,
        Some(model) => {
            // The prompt options come from the trace; only where the model
            // runs comes from the settings (no repository: it may be gone).
            let settings = resolve(None, model.flags()?)?.settings();
            let (backend, _) = load_model(&settings)?;
            if backend.describe() != trace.backend {
                note!(
                    "recorded with `{}`, replaying with `{}`",
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
        bail!(
            "not reproduced (trace written by smart-commit {}, this is {})",
            trace.tool_version,
            env!("CARGO_PKG_VERSION")
        );
    }
    println!(
        "reproduced: {} attempt(s), {:?} message",
        replay.generation.attempts.len(),
        replay.generation.source
    );
    println!("{}", replay.generation.message);
    Ok(())
}

pub(super) fn hardware() {
    print!("{}", Hardware::detect(&hardware_sources()).inventory());
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
