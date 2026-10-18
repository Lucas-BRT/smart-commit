use std::error::Error;
use std::process::ExitCode;

use npu_commit::context::RepoContext;
use npu_commit::diff;
use npu_commit::git::Repo;
use npu_commit::prompt::{self, PromptOptions};

const USAGE: &str = "usage: npu-commit context [PATH]\n       npu-commit prompt [PATH]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["context"] => run_context("."),
        ["context", path] => run_context(path),
        ["prompt"] => run_prompt("."),
        ["prompt", path] => run_prompt(path),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_context(path: &str) -> Result<(), Box<dyn Error>> {
    let context = RepoContext::collect(&Repo::discover(path)?)?;
    print_context(&context);
    Ok(())
}

/// Prints the prompt the model would get for the staged changes.
fn run_prompt(path: &str) -> Result<(), Box<dyn Error>> {
    let repo = Repo::discover(path)?;
    let context = RepoContext::collect(&repo)?;
    let files = repo.staged_diff()?;
    if files.is_empty() {
        return Err("nothing staged".into());
    }
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
