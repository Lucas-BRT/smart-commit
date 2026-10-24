//! `smart-commit hook`: the `prepare-commit-msg` git hook.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use smart_commit::git::Repo;
use smart_commit::hook;
use std::error::Error;

use super::args::BackendArgs;
use super::suggest::generate_message;

const HOOK_NAME: &str = "prepare-commit-msg";
const HOOK_BACKUP: &str = "prepare-commit-msg.smart-commit-backup";

#[derive(Debug, Subcommand)]
pub(super) enum HookCommand {
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

pub(super) fn run(command: HookCommand) -> Result<(), Box<dyn Error>> {
    match command {
        HookCommand::Install { path, force } => install(&path, force),
        HookCommand::Uninstall { path } => uninstall(&path),
        HookCommand::Run {
            message_file,
            source,
            sha: _,
            backend,
        } => {
            fill(&message_file, source.as_deref(), &backend);
            Ok(())
        }
    }
}

fn install(path: &Path, force: bool) -> Result<(), Box<dyn Error>> {
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

fn uninstall(path: &Path) -> Result<(), Box<dyn Error>> {
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
fn fill(message_file: &Path, source: Option<&str>, backend: &BackendArgs) {
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
