//! `smart-commit model`: list the recommended models and switch between
//! them, or to a model file on disk.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use clap::Subcommand;
use smart_commit::config::{self, Origin, Resolved};
use smart_commit::models::{self, Model};
use smart_commit::paths;
use smart_commit::tui::models::{Action, Entry, Picker};
use smart_commit::tui::{self, Screen};

use super::settings::{env, resolve, stop_kept_after};
use super::ui::note;

#[derive(Debug, Subcommand)]
pub(super) enum ModelCommand {
    /// The recommended models, which are installed and which is in use.
    List,
    /// Download a recommended model from Hugging Face and use it, or use a
    /// GGUF file (or an OpenVINO model directory) from disk.
    Use {
        /// A model id from `smart-commit model list`, or a path.
        model: String,
    },
}

pub(super) fn run(command: Option<ModelCommand>) -> Result<()> {
    match command {
        None if tui::interactive() => pick(),
        None | Some(ModelCommand::List) => list(),
        Some(ModelCommand::Use { model }) => match models::find(&model) {
            Some(model) => use_recommended(model),
            None => use_file(&model),
        },
    }
}

fn list() -> Result<()> {
    let resolved = resolve(None, Vec::new())?;
    let settings = resolved.settings();
    let models_dir = paths::default_models_dir(env);
    println!(
        "  {:<22} {:>7}  {:<11} {:<13} installed",
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
            "  {:<22} {:>7}  {:<11} {:<13} {}{mark}",
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
        println!("  {:<22} {}{openvino}", model.id, model.summary);
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
        "Models come from Hugging Face, pinned and checked: their publishers' files,\n\
         or smart-commit's own fine-tunes (smart-commit-*).\n\
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

/// The model picker, in a terminal; what it chooses is then used as with
/// `model use`.
fn pick() -> Result<()> {
    let dir = paths::default_models_dir(env).context("HOME is not set")?;
    let mut picker = picker(&dir)?;
    let base = hugging_face();
    let mut screen = Screen::new()?;
    let mut download: Option<Download> = None;
    let choice = loop {
        screen.draw(|frame| picker.render(frame))?;
        if let Some(current) = &download {
            while let Ok(bytes) = current.progress.try_recv() {
                picker.progress(bytes);
            }
            if current.worker.is_finished() {
                let Download { model, worker, .. } = download.take().expect("checked");
                match worker.join() {
                    Ok(Ok(_)) => break Some(Action::Use(model)),
                    Ok(Err(err)) => picker.download_failed(err.to_string()),
                    Err(_) => picker.download_failed("the download crashed".to_owned()),
                }
                continue;
            }
            // Ctrl+C stops waiting; curl resumes the download next time.
            if let Some(key) = screen.key_within(Duration::from_millis(200))?
                && picker.on_key(key) == Action::Quit
            {
                break None;
            }
            continue;
        }
        match picker.on_key(screen.key()?) {
            Action::None => {}
            Action::Quit => break None,
            Action::Download(model) => {
                let (sender, progress) = mpsc::channel();
                let (dir, base) = (dir.clone(), base.clone());
                let worker = std::thread::spawn(move || {
                    models::install_gguf(model, &dir, &base, |bytes| {
                        let _ = sender.send(bytes);
                    })
                });
                download = Some(Download {
                    model,
                    progress,
                    worker,
                });
            }
            action => break Some(action),
        }
    };
    drop(screen);
    match choice {
        Some(Action::Use(model)) => use_recommended(model),
        Some(Action::UseFile(path)) => use_file(&path.to_string_lossy()),
        _ => Ok(()),
    }
}

/// What the picker chose in the setup, which downloads later.
pub(super) enum Chosen {
    Model(&'static Model),
    File(PathBuf),
}

/// The picker on `screen`, choosing without downloading.
pub(super) fn choose(screen: &mut Screen) -> Result<Option<Chosen>> {
    let dir = paths::default_models_dir(env).context("HOME is not set")?;
    let mut picker = picker(&dir)?;
    loop {
        screen.draw(|frame| picker.render(frame))?;
        match picker.on_key(screen.key()?) {
            Action::None => {}
            Action::Quit => return Ok(None),
            Action::Use(model) | Action::Download(model) => return Ok(Some(Chosen::Model(model))),
            Action::UseFile(path) => return Ok(Some(Chosen::File(path))),
        }
    }
}

/// The picker, on the models in `dir`.
fn picker(dir: &Path) -> Result<Picker> {
    let resolved = resolve(None, Vec::new())?;
    let settings = resolved.settings();
    let entries = models::REGISTRY
        .iter()
        .map(|model| {
            let parts = installed(model, dir);
            Entry {
                model,
                gguf: parts.contains(&"gguf"),
                openvino: parts.contains(&"openvino"),
            }
        })
        .collect();
    let custom = ["model.gguf", "model.dir"].iter().find_map(|key| {
        resolved
            .get(key)
            .filter(|(_, origin)| *origin != Origin::Default)
            .map(|(value, _)| format!("{key} = {value} replaces the model on its devices"))
    });
    Ok(Picker::new(entries, settings.model.id, custom, env("HOME")))
}

/// A download running in its own thread, so the screen keeps drawing.
struct Download {
    model: &'static Model,
    progress: mpsc::Receiver<u64>,
    worker: std::thread::JoinHandle<Result<PathBuf, models::ModelError>>,
}

/// Where models are downloaded from: Hugging Face, unless a test serves
/// them.
fn hugging_face() -> String {
    env("SMART_COMMIT_HF_BASE").unwrap_or_else(|| models::HUGGING_FACE.to_owned())
}

fn use_recommended(model: &'static Model) -> Result<()> {
    let dir = paths::default_models_dir(env).context("HOME is not set")?;
    let base = hugging_face();
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

    for line in select(model)? {
        println!("{line}");
    }
    let resolved = resolve(None, Vec::new())?;
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

/// Makes `model` the one in use (model.name, without model.gguf or
/// model.dir pointing elsewhere); returns what changed.
pub(super) fn select(model: &'static Model) -> Result<Vec<String>> {
    let path = global_config()?;
    let origin = Origin::Global(path.clone());
    config::set_in_file(&path, &origin, "model.name", model.id, None)?;
    let mut lines = vec![format!("model.name = {}  ({origin})", model.id)];
    lines.extend(stop_kept_after("model.name"));
    for key in ["model.gguf", "model.dir"] {
        if config::unset_in_file(&path, key)? {
            lines.push(format!(
                "{key} removed, so {} is used on every device",
                model.id
            ));
        }
    }
    Ok(lines)
}

fn use_file(arg: &str) -> Result<()> {
    let path = Path::new(arg);
    if !path.exists() {
        bail!(
            "{arg} is neither a recommended model ({}) nor a file",
            models::IDS.join(", ")
        );
    }
    let path: PathBuf = path.canonicalize()?;
    let key = if path.is_dir() {
        if !path.join("openvino_model.xml").is_file() {
            bail!(
                "{} has no openvino_model.xml: not an OpenVINO model",
                path.display()
            );
        }
        "model.dir"
    } else {
        models::check_gguf(&path)?;
        "model.gguf"
    };
    for line in select_file(&path, key)? {
        println!("{line}");
    }
    let resolved = resolve(None, Vec::new())?;
    warn_overridden(&resolved, &[key]);
    suggest_bench();
    Ok(())
}

/// Sets `key` (model.gguf or model.dir) to `path`; returns what changed.
fn select_file(path: &Path, key: &str) -> Result<Vec<String>> {
    let config_path = global_config()?;
    let origin = Origin::Global(config_path.clone());
    let text = path.to_string_lossy();
    config::set_in_file(&config_path, &origin, key, &text, None)?;
    let stopped = stop_kept_after(key);
    let devices = if key == "model.gguf" {
        "vulkan and cpu"
    } else {
        "npu, intel-gpu and openvino-cpu"
    };
    let mut lines = vec![
        format!("{key} = {text}  ({origin})"),
        format!(
            "{devices} now run this file, unverified: smart-commit did not choose it and \
             cannot check it. `smart-commit model use <id>` goes back to a recommended model."
        ),
    ];
    lines.extend(stopped);
    Ok(lines)
}

/// Uses a model file or directory chosen in the setup's picker.
pub(super) fn select_path(path: &Path) -> Result<Vec<String>> {
    let path = path.canonicalize()?;
    if path.is_dir() {
        if !path.join("openvino_model.xml").is_file() {
            bail!("{} has no openvino_model.xml", path.display());
        }
        select_file(&path, "model.dir")
    } else {
        models::check_gguf(&path)?;
        select_file(&path, "model.gguf")
    }
}

fn global_config() -> Result<PathBuf> {
    paths::default_config_file(env).context("HOME is not set")
}

/// A note for each of `keys` still set by the environment or a flag.
fn warn_overridden(resolved: &Resolved, keys: &[&str]) {
    for key in keys {
        if let Some((value, origin @ Origin::Env(_))) = resolved.get(key) {
            note!("{key} = {value} from the {origin} overrides the config.");
        }
    }
}

fn suggest_bench() {
    if super::benchmark::last_report().is_some() {
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
