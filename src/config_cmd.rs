//! `smart-commit config`: read and change settings.

use std::error::Error;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use smart_commit::config::{self, Origin, Resolved, Sources};
use smart_commit::git::Repo;
use smart_commit::paths;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show every setting with its value, after all layers.
    List {
        /// Also show where each value comes from.
        #[arg(long)]
        show_origin: bool,
        #[command(flatten)]
        at: At,
    },
    /// Print the value of a setting, after all layers.
    Get {
        key: String,
        #[command(flatten)]
        at: At,
    },
    /// Change a setting in the global config, or the repository's.
    Set {
        key: String,
        /// Checked against the setting's type; may start with `-`.
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[command(flatten)]
        target: Target,
    },
    /// Remove a setting from a config file, so the next layer applies.
    Unset {
        key: String,
        #[command(flatten)]
        target: Target,
    },
    /// Print the path of the global config file, or the repository's.
    Path {
        #[command(flatten)]
        target: Target,
    },
    /// Write a config file with every setting commented out, with its
    /// description and default.
    Init {
        #[command(flatten)]
        target: Target,
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
}

/// The repository whose `.smart-commit.toml` applies, if any.
#[derive(Debug, Args)]
pub struct At {
    #[arg(short = 'C', long = "path", default_value = ".")]
    path: PathBuf,
}

/// Which config file to change.
#[derive(Debug, Args)]
pub struct Target {
    /// The repository's .smart-commit.toml (shared through git) instead of
    /// the global config; only style.* settings are allowed there.
    #[arg(long)]
    repo: bool,
    #[arg(short = 'C', long = "path", default_value = ".")]
    path: PathBuf,
}

impl Target {
    fn file(&self) -> Result<(PathBuf, Origin), Box<dyn Error>> {
        if self.repo {
            let path = Repo::discover(&self.path)?.root().join(config::REPO_FILE);
            return Ok((path.clone(), Origin::Repo(path)));
        }
        let path = paths::default_config_file(env).ok_or("HOME is not set")?;
        Ok((path.clone(), Origin::Global(path)))
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The repository at `path`, if there is one: outside a repository only
/// the global config applies.
fn resolve_at(path: &PathBuf) -> Result<Resolved, Box<dyn Error>> {
    let repo = Repo::discover(path).ok();
    Ok(crate::resolve(repo.as_ref(), Vec::new())?)
}

pub fn run(command: ConfigCommand) -> Result<(), Box<dyn Error>> {
    match command {
        ConfigCommand::List { show_origin, at } => {
            let resolved = resolve_at(&at.path)?;
            for (key, entry) in resolved.entries() {
                match entry {
                    Some((value, origin)) if show_origin => {
                        println!("{} = {value}  ({origin})", key.name);
                    }
                    Some((value, _)) => println!("{} = {value}", key.name),
                    None => println!("{} = (unset)", key.name),
                }
            }
        }
        ConfigCommand::Get { key, at } => {
            config::key(&key)?;
            let resolved = resolve_at(&at.path)?;
            let (value, _) = resolved.get(&key).ok_or_else(|| {
                if key == "model.dir" {
                    format!(
                        "model.dir has no value: {} has no OpenVINO version",
                        resolved.settings().model.id
                    )
                } else {
                    format!("{key} has no value (its default needs $HOME)")
                }
            })?;
            println!("{value}");
        }
        ConfigCommand::Set { key, value, target } => {
            let (path, origin) = target.file()?;
            let home = env("HOME");
            let value = config::set_in_file(&path, &origin, &key, &value, home.as_deref())?;
            println!("{key} = {value}  ({origin})");
            if let Some(note) = crate::stop_kept_after(&key) {
                println!("{note}");
            }
            warn_if_shadowed(&key, &target, &origin)?;
        }
        ConfigCommand::Unset { key, target } => {
            let (path, origin) = target.file()?;
            if config::unset_in_file(&path, &key)? {
                println!("removed {key} from {}", path.display());
                if let Some(note) = crate::stop_kept_after(&key) {
                    println!("{note}");
                }
            } else {
                println!("{key} is not set in {}", path.display());
            }
            warn_if_shadowed(&key, &target, &origin)?;
        }
        ConfigCommand::Path { target } => println!("{}", target.file()?.0.display()),
        ConfigCommand::Init { target, force } => {
            let (path, _) = target.file()?;
            if path.exists() && !force {
                return Err(format!(
                    "{} already exists; use --force to overwrite it",
                    path.display()
                )
                .into());
            }
            let template = config::template(target.repo, &built_in_defaults()?);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, template)?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// Defaults alone: no files, and of the environment only what locates the
/// home and XDG directories.
fn built_in_defaults() -> Result<Resolved, Box<dyn Error>> {
    let env = |name: &str| {
        (name == "HOME" || name.starts_with("XDG_"))
            .then(|| std::env::var(name).ok())
            .flatten()
    };
    Ok(Resolved::resolve(&Sources {
        global: None,
        repo: None,
        env: &env,
        flags: Vec::new(),
    })?)
}

/// After a change, says so if a higher layer still decides the value.
fn warn_if_shadowed(key: &str, target: &Target, changed: &Origin) -> Result<(), Box<dyn Error>> {
    let resolved = resolve_at(&target.path)?;
    if let Some((value, origin)) = resolved.get(key) {
        let shadowed = match origin {
            Origin::Env(_) | Origin::Flag => true,
            Origin::Repo(_) => matches!(changed, Origin::Global(_)),
            Origin::Global(_) | Origin::Default => false,
        };
        if shadowed {
            eprintln!("note: {key} is still {value}, from the {origin}");
        }
    }
    Ok(())
}
