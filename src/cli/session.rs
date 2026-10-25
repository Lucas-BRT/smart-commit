//! What a message is made from, and the model that writes it.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use smart_commit::backend::device::Target;
use smart_commit::backend::{Backend, GenerationConfig, MockBackend, NoModel};
use smart_commit::config::{BackendKind, Origin, Resolved, Value};
use smart_commit::context::RepoContext;
use smart_commit::diff::FileDiff;
use smart_commit::generate::{self, Generation};
use smart_commit::git::Repo;
use smart_commit::prompt::PromptOptions;
use smart_commit::trace::Trace;

use super::args::BackendArgs;
use super::load::load_model;
use super::settings::{resolve, warn_renamed};

/// A loaded backend, with the model and the device for people:
/// `qwen2.5-coder-1.5b on cpu`.
pub(super) type Loaded = (Box<dyn Backend>, String);

/// What a message is made from, and the model once it is needed, kept to
/// ask again.
pub(super) struct Session {
    pub(super) context: RepoContext,
    files: Vec<FileDiff>,
    /// Loaded on the first message that needs it: lockfiles alone need none.
    backend: OnceCell<Loaded>,
    resolved: Resolved,
    mock_responses: Vec<String>,
    options: PromptOptions,
    config: GenerationConfig,
    trace: Option<PathBuf>,
}

impl Session {
    pub(super) fn start(repo: &Repo, args: &BackendArgs) -> Result<Self> {
        let resolved = resolve(Some(repo), args.flags()?)?;
        warn_renamed(&resolved);
        let settings = resolved.settings();
        let context = RepoContext::collect_with_history(repo, settings.history_depth)?;
        let files = staged_files(repo, &context)?;
        Ok(Self {
            context,
            files,
            backend: OnceCell::new(),
            options: settings.prompt_options(),
            config: settings.generation_config(),
            resolved,
            mock_responses: args.mock_responses.clone(),
            trace: args.trace.clone(),
        })
    }

    /// The backend, loaded the first time.
    fn backend(&self) -> Result<&Loaded> {
        if let Some(loaded) = self.backend.get() {
            return Ok(loaded);
        }
        let loaded = start_backend(&self.resolved, &self.mock_responses)?;
        Ok(self.backend.get_or_init(|| loaded))
    }

    /// What wrote the messages, for people.
    pub(super) fn running(&self) -> String {
        match self.backend.get() {
            Some((_, label)) => label.clone(),
            None => "no model".to_owned(),
        }
    }

    /// A message other than those in `avoid`, writing the trace if asked.
    pub(super) fn generate(&self, avoid: &[String]) -> Result<Generation> {
        let backend: &dyn Backend = if generate::needs_model(&self.context, &self.files, avoid) {
            self.backend()?.0.as_ref()
        } else {
            &NoModel
        };
        let generation = generate::generate_avoiding(
            &self.context,
            &self.files,
            backend,
            &self.options,
            &self.config,
            avoid,
        )?;
        if let Some(path) = &self.trace {
            let trace = Trace::new(
                &self.context,
                &self.files,
                backend,
                &self.options,
                &self.config,
                &generation,
            );
            std::fs::write(path, trace.to_json())
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
        Ok(generation)
    }
}

/// The backend `resolved` asks for, and its model and device for people.
pub(super) fn start_backend(resolved: &Resolved, mock_responses: &[String]) -> Result<Loaded> {
    let settings = resolved.settings();
    Ok(match settings.backend {
        BackendKind::Mock => (
            Box::new(MockBackend::new(mock_responses.to_vec())),
            "the mock backend".to_owned(),
        ),
        _ => {
            let (backend, target) = load_model(&settings)?;
            (
                backend,
                format!("{} on {target}", model_label(resolved, target)),
            )
        }
    })
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

/// The staged diff; none is an error, unless git prepared the message (a
/// merge resolved to what HEAD already has stages nothing, yet commits).
pub(super) fn staged_files(repo: &Repo, context: &RepoContext) -> Result<Vec<FileDiff>> {
    let files = repo.staged_diff()?;
    if files.is_empty() && context.prepared.is_none() {
        bail!("nothing staged");
    }
    Ok(files)
}
