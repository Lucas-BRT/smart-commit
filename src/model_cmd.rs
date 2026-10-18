//! `smart-commit model`: list the recommended models and switch between
//! them, or to a model file on disk.

use std::error::Error;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::Subcommand;
use smart_commit::config::{self, Origin, Resolved};
use smart_commit::models::{self, Model};
use smart_commit::paths;

#[derive(Debug, Subcommand)]
pub enum ModelCommand {
    /// The recommended models, which are installed and which is in use.
    List,
    /// Download a recommended model from Hugging Face and use it, or use a
    /// GGUF file (or an OpenVINO model directory) from disk.
    Use {
        /// A model id from `smart-commit model list`, or a path.
        model: String,
    },
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn run(command: Option<ModelCommand>) -> Result<(), Box<dyn Error>> {
    match command {
        None | Some(ModelCommand::List) => list(),
        Some(ModelCommand::Use { model }) => match models::find(&model) {
            Some(model) => use_recommended(model),
            None => use_file(&model),
        },
    }
}

fn list() -> Result<(), Box<dyn Error>> {
    let resolved = crate::resolve(None, Vec::new())?;
    let settings = resolved.settings();
    let models_dir = paths::default_models_dir(env);
    println!(
        "  {:<20} {:>7}  {:<11} {:<13} installed",
        "model", "size", "licence", "status"
    );
    for model in models::REGISTRY {
        let installed = models_dir
            .as_deref()
            .map(|dir| installed(model, dir))
            .unwrap_or_default();
        let mark = if model.id == settings.model.id {
            "  <- in use"
        } else {
            ""
        };
        println!(
            "  {:<20} {:>7}  {:<11} {:<13} {}{mark}",
            model.id,
            models::size(model.gguf.size),
            model.license,
            model.status.to_string(),
            if installed.is_empty() {
                "-".to_owned()
            } else {
                installed.join(", ")
            }
        );
    }
    println!();
    for model in models::REGISTRY {
        let openvino = if model.openvino.is_some() {
            ""
        } else {
            "; llama.cpp devices only (vulkan, cpu)"
        };
        println!("  {:<20} {}{openvino}", model.id, model.summary);
    }
    println!();
    let origin = resolved
        .get("model.name")
        .map(|(_, o)| o.to_string())
        .unwrap_or_default();
    println!("In use: {} (model.name, {origin}).", settings.model.id);
    for key in ["model.gguf", "model.dir"] {
        if let Some((value, origin)) = resolved.get(key)
            && *origin != Origin::Default
        {
            println!("{key} = {value} ({origin}) replaces it on its devices.");
        }
    }
    println!(
        "Models come from their publishers on Hugging Face, pinned and checked.\n\
         To change: smart-commit model use <id>, or a GGUF file: smart-commit model use FILE"
    );
    Ok(())
}

/// What of `model` is in `dir`: its GGUF, its OpenVINO export.
fn installed(model: &Model, dir: &Path) -> Vec<&'static str> {
    let mut parts = Vec::new();
    if models::is_installed(&model.gguf, &dir.join(model.gguf.file)) {
        parts.push("gguf");
    }
    if model
        .openvino
        .is_some_and(|name| dir.join(name).join("openvino_model.xml").is_file())
    {
        parts.push("openvino");
    }
    parts
}

fn use_recommended(model: &'static Model) -> Result<(), Box<dyn Error>> {
    let dir = paths::default_models_dir(env).ok_or("HOME is not set")?;
    // Only tests serve the models from elsewhere.
    let base = env("SMART_COMMIT_HF_BASE").unwrap_or_else(|| models::HUGGING_FACE.to_owned());
    let file = dir.join(model.gguf.file);
    if models::is_installed(&model.gguf, &file) {
        eprintln!("{} is already installed: {}", model.name, file.display());
    } else {
        eprintln!(
            "downloading {} ({}, {}) from {}/{}",
            model.name,
            models::size(model.gguf.size),
            model.license,
            base,
            model.gguf.repo
        );
        let progress = Progress::new(model.gguf.size);
        let file = models::install_gguf(model, &dir, &base, |n| progress.show(n))?;
        progress.done();
        eprintln!("checked against the pinned SHA-256: {}", file.display());
    }

    let path = global_config()?;
    let origin = Origin::Global(path.clone());
    config::set_in_file(&path, &origin, "model.name", model.id, None)?;
    println!("model.name = {}  ({origin})", model.id);
    for key in ["model.gguf", "model.dir"] {
        if config::unset_in_file(&path, key)? {
            println!("{key} removed, so {} is used on every device", model.id);
        }
    }
    let resolved = crate::resolve(None, Vec::new())?;
    warn_overridden(&resolved, &["model.name", "model.gguf", "model.dir"]);
    match model.openvino {
        None => println!(
            "note: {} has no OpenVINO version: npu, intel-gpu and openvino-cpu cannot run it.",
            model.id
        ),
        Some(name) if !dir.join(name).join("openvino_model.xml").is_file() => println!(
            "note: its OpenVINO version (npu, intel-gpu, openvino-cpu) is not installed; \
             `smart-commit setup` exports it on machines with OpenVINO."
        ),
        Some(_) => {}
    }
    suggest_bench();
    Ok(())
}

fn use_file(arg: &str) -> Result<(), Box<dyn Error>> {
    let path = Path::new(arg);
    if !path.exists() {
        return Err(format!(
            "{arg} is neither a recommended model ({}) nor a file",
            models::IDS.join(", ")
        )
        .into());
    }
    let path: PathBuf = path.canonicalize()?;
    let key = if path.is_dir() {
        if !path.join("openvino_model.xml").is_file() {
            return Err(format!(
                "{} has no openvino_model.xml: not an OpenVINO model",
                path.display()
            )
            .into());
        }
        "model.dir"
    } else {
        models::check_gguf(&path)?;
        "model.gguf"
    };
    let config_path = global_config()?;
    let origin = Origin::Global(config_path.clone());
    let text = path.to_string_lossy();
    config::set_in_file(&config_path, &origin, key, &text, None)?;
    println!("{key} = {text}  ({origin})");
    let devices = if key == "model.gguf" {
        "vulkan and cpu"
    } else {
        "npu, intel-gpu and openvino-cpu"
    };
    println!(
        "{devices} now run this file, unverified: smart-commit did not choose it and \
         cannot check it. `smart-commit model use <id>` goes back to a recommended model."
    );
    let resolved = crate::resolve(None, Vec::new())?;
    warn_overridden(&resolved, &[key]);
    suggest_bench();
    Ok(())
}

fn global_config() -> Result<PathBuf, Box<dyn Error>> {
    Ok(paths::default_config_file(env).ok_or("HOME is not set")?)
}

/// A note for each of `keys` still set by the environment or a flag.
fn warn_overridden(resolved: &Resolved, keys: &[&str]) {
    for key in keys {
        if let Some((value, origin @ Origin::Env(_))) = resolved.get(key) {
            eprintln!("note: {key} = {value} from the {origin} overrides the config.");
        }
    }
}

fn suggest_bench() {
    if crate::bench_cmd::last_report().is_some() {
        println!("The last benchmark measured the previous model: run `smart-commit bench` again.");
    }
}

/// Download progress on stderr, on one line, when it is a terminal.
struct Progress {
    total: u64,
    terminal: bool,
}

impl Progress {
    fn new(total: u64) -> Self {
        Self {
            total,
            terminal: std::io::stderr().is_terminal(),
        }
    }

    fn show(&self, bytes: u64) {
        if self.terminal {
            let percent = (bytes * 100).checked_div(self.total).unwrap_or(0).min(100);
            eprint!(
                "\r  {} of {} ({percent}%)   ",
                models::size(bytes),
                models::size(self.total)
            );
            let _ = std::io::stderr().flush();
        }
    }

    fn done(&self) {
        if self.terminal {
            eprintln!();
        }
    }
}
