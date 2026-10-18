//! Settings from config files, the environment and flags.
//!
//! Precedence, highest first: command-line flags, environment variables,
//! the repository's `.smart-commit.toml`, the global
//! `$XDG_CONFIG_HOME/smart-commit/config.toml`, and the defaults.
//!
//! Every key is declared once in [`KEYS`], which drives parsing, validation,
//! `config set`, listing and the `config init` template alike. Unknown keys
//! and invalid values are errors, never silently ignored. Keys about the
//! machine (`backend`, `model.*`) can only be set globally: cloning a
//! repository must not change which model runs, or from where.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table};

use crate::backend::GenerationConfig;
use crate::genai::{Device, GenaiOptions, NpuTuning};
use crate::llamacpp::LlamaCppOptions;
use crate::paths;
use crate::prompt::{DEFAULT_MAX_PROMPT_TOKENS, PromptOptions};
use crate::style::{Convention, Language};

/// The per-repository file, at the root of the work tree.
pub const REPO_FILE: &str = ".smart-commit.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Choice(&'static [&'static str]),
    Count {
        min: usize,
        max: usize,
    },
    /// Absolute, or starting with `~/`.
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Global file, repository file, environment or flag.
    Any,
    /// Not in the repository file.
    User,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    pub name: &'static str,
    pub kind: Kind,
    pub scope: Scope,
    pub env: Option<&'static str>,
    pub description: &'static str,
}

pub const KEYS: &[Key] = &[
    Key {
        name: "backend",
        kind: Kind::Choice(&["auto", "openvino", "llamacpp", "mock"]),
        scope: Scope::User,
        env: Some("SMART_COMMIT_BACKEND"),
        description: "What runs the model: `openvino` (Intel NPU, GPU and CPU), `llamacpp` \
                      (any GPU through Vulkan), `auto` (whichever model.device needs) or \
                      `mock`, which replies with --mock-response (for tests).",
    },
    Key {
        name: "model.device",
        kind: Kind::Choice(&["auto", "npu", "gpu", "cpu", "vulkan"]),
        scope: Scope::User,
        env: Some("SMART_COMMIT_DEVICE"),
        description: "Where the model runs: `npu`, `gpu` (Intel) and `cpu` through OpenVINO, \
                      `vulkan` (AMD, NVIDIA or Intel GPU) through llama.cpp, or `auto`: \
                      the fastest in the last `smart-commit bench`, else the first that works \
                      of gpu, npu, vulkan, cpu.",
    },
    Key {
        name: "model.dir",
        kind: Kind::Path,
        scope: Scope::User,
        env: Some("SMART_COMMIT_MODEL_DIR"),
        description: "The OpenVINO model, downloaded by `smart-commit setup`.",
    },
    Key {
        name: "model.cache_dir",
        kind: Kind::Path,
        scope: Scope::User,
        env: Some("SMART_COMMIT_CACHE_DIR"),
        description: "Where OpenVINO keeps compiled models, so loads after the first are fast.",
    },
    Key {
        name: "model.gguf",
        kind: Kind::Path,
        scope: Scope::User,
        env: Some("SMART_COMMIT_GGUF"),
        description: "The GGUF model of the llama.cpp backend, downloaded by `smart-commit setup`.",
    },
    Key {
        name: "model.llama_server",
        kind: Kind::Path,
        scope: Scope::User,
        env: Some("SMART_COMMIT_LLAMA_SERVER"),
        description: "The llama-server binary, installed by `smart-commit setup`.",
    },
    Key {
        name: "model.vulkan_device",
        kind: Kind::Count { min: 0, max: 15 },
        scope: Scope::User,
        env: None,
        description: "Which Vulkan GPU llama.cpp uses, as `llama-server --list-devices` \
                      numbers them (0 is Vulkan0).",
    },
    Key {
        name: "model.npu_prefill_chunk",
        kind: Kind::Count {
            min: 512,
            max: 8192,
        },
        scope: Scope::User,
        env: None,
        description: "NPU only: prompt tokens processed per step; below \
                      model.max_prompt_tokens (and dividing it) the NPU pays only for the \
                      prompt's real length. Smaller chunks fail to compile with driver 1.38.",
    },
    Key {
        name: "model.npu_generate_hint",
        kind: Kind::Choice(&["best_perf", "fast_compile"]),
        scope: Scope::User,
        env: None,
        description: "NPU only: `best_perf` compiles slower (once, then cached) for faster \
                      tokens.",
    },
    Key {
        name: "model.max_prompt_tokens",
        kind: Kind::Count {
            min: 256,
            max: 8192,
        },
        scope: Scope::User,
        env: None,
        description: "Prompt length the NPU compiles for; longer prompts see more of the diff \
                      but are slower.",
    },
    Key {
        name: "model.max_new_tokens",
        kind: Kind::Count { min: 16, max: 1024 },
        scope: Scope::User,
        env: None,
        description: "Longest reply, in tokens.",
    },
    Key {
        name: "style.convention",
        kind: Kind::Choice(&["auto", "conventional", "gitmoji", "free"]),
        scope: Scope::Any,
        env: None,
        description: "Commit convention; `auto` follows the history.",
    },
    Key {
        name: "style.language",
        kind: Kind::Choice(&["auto", "english", "portuguese"]),
        scope: Scope::Any,
        env: None,
        description: "Message language; `auto` follows the history.",
    },
    Key {
        name: "style.default_convention",
        kind: Kind::Choice(&["conventional", "gitmoji", "free"]),
        scope: Scope::Any,
        env: None,
        description: "Convention when `auto` has no history to learn from.",
    },
    Key {
        name: "style.default_language",
        kind: Kind::Choice(&["english", "portuguese"]),
        scope: Scope::Any,
        env: None,
        description: "Language when `auto` cannot tell from the history.",
    },
    Key {
        name: "style.history_depth",
        kind: Kind::Count { min: 1, max: 500 },
        scope: Scope::Any,
        env: None,
        description: "How many recent commits the style is learned from.",
    },
];

pub fn key(name: &str) -> Result<&'static Key, ConfigError> {
    KEYS.iter()
        .find(|k| k.name == name)
        .ok_or_else(|| ConfigError::UnknownKey {
            key: name.to_owned(),
            location: None,
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Count(usize),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.write_str(text),
            Self::Count(n) => write!(f, "{n}"),
        }
    }
}

/// Where a setting came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Default,
    Global(PathBuf),
    Repo(PathBuf),
    Env(&'static str),
    Flag,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::Global(path) => write!(f, "global {}", path.display()),
            Self::Repo(path) => write!(f, "repository {}", path.display()),
            Self::Env(var) => write!(f, "environment {var}"),
            Self::Flag => f.write_str("command line"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown key `{key}`{}; known keys: {}", at(location), known_keys())]
    UnknownKey {
        key: String,
        location: Option<String>,
    },
    #[error("`{key}`{}: {problem}", at(location))]
    InvalidValue {
        key: String,
        location: Option<String>,
        problem: String,
    },
    #[error("`{key}` can only be set in the global config, not in {path}")]
    UserOnly { key: String, path: String },
    #[error("cannot parse {path}: {problem}")]
    Syntax { path: String, problem: String },
    #[error("{0}")]
    Io(String),
}

fn at(location: &Option<String>) -> String {
    location
        .as_ref()
        .map(|l| format!(" in {l}"))
        .unwrap_or_default()
}

fn known_keys() -> String {
    KEYS.iter().map(|k| k.name).collect::<Vec<_>>().join(", ")
}

/// Checks a value given as text (environment, flag, `config set`).
pub fn parse(key: &Key, text: &str, home: Option<&str>) -> Result<Value, String> {
    match key.kind {
        Kind::Choice(choices) => {
            if choices.contains(&text) {
                Ok(Value::Text(text.to_owned()))
            } else {
                Err(format!(
                    "expected one of {}, got {text:?}",
                    choices.join(", ")
                ))
            }
        }
        Kind::Count { min, max } => match text.parse::<usize>() {
            Ok(n) if (min..=max).contains(&n) => Ok(Value::Count(n)),
            _ => Err(format!(
                "expected a whole number from {min} to {max}, got {text:?}"
            )),
        },
        Kind::Path => expand_path(text, home)
            .map(|p| Value::Text(p.to_string_lossy().into_owned()))
            .ok_or_else(|| {
                format!("expected an absolute path or one starting with ~/, got {text:?}")
            }),
    }
}

fn expand_path(text: &str, home: Option<&str>) -> Option<PathBuf> {
    if let Some(rest) = text.strip_prefix("~/") {
        return home
            .filter(|h| !h.is_empty())
            .map(|h| Path::new(h).join(rest));
    }
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

/// Checks a value read from a file, which must have the right TOML type.
fn parse_item(key: &Key, item: &Item, home: Option<&str>) -> Result<Value, String> {
    match (key.kind, item.as_value()) {
        (Kind::Count { .. }, Some(value)) => match value.as_integer() {
            Some(n) if n >= 0 => parse(key, &n.to_string(), home),
            _ => Err(format!(
                "expected a whole number, got {}",
                value.to_string().trim()
            )),
        },
        (_, Some(value)) => match value.as_str() {
            Some(text) => parse(key, text, home),
            None => Err(format!(
                "expected a string, got {}",
                value.to_string().trim()
            )),
        },
        (_, None) => Err("expected a value, not a table".to_owned()),
    }
}

/// The values of one config file, checked against [`KEYS`].
pub fn read_file(
    path: &Path,
    origin: &Origin,
    home: Option<&str>,
) -> Result<Vec<(&'static Key, Value)>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(ConfigError::Io(format!(
                "cannot read {}: {err}",
                path.display()
            )));
        }
    };
    parse_file(&text, path, origin, home)
}

pub fn parse_file(
    text: &str,
    path: &Path,
    origin: &Origin,
    home: Option<&str>,
) -> Result<Vec<(&'static Key, Value)>, ConfigError> {
    let location = path.display().to_string();
    let doc: DocumentMut =
        text.parse()
            .map_err(|err: toml_edit::TomlError| ConfigError::Syntax {
                path: location.clone(),
                problem: err.message().to_owned(),
            })?;
    let mut values = Vec::new();
    for (name, item) in flatten(doc.as_table()) {
        let key = KEYS
            .iter()
            .find(|k| k.name == name)
            .ok_or_else(|| ConfigError::UnknownKey {
                key: name.clone(),
                location: Some(location.clone()),
            })?;
        if key.scope == Scope::User && matches!(origin, Origin::Repo(_)) {
            return Err(ConfigError::UserOnly {
                key: name,
                path: location,
            });
        }
        let value = parse_item(key, item, home).map_err(|problem| ConfigError::InvalidValue {
            key: name.clone(),
            location: Some(location.clone()),
            problem,
        })?;
        values.push((key, value));
    }
    Ok(values)
}

/// `section.key` names of every value, one table level deep (the layout
/// [`KEYS`] uses); deeper tables come out as unknown keys.
fn flatten(table: &Table) -> Vec<(String, &Item)> {
    let mut out = Vec::new();
    for (name, item) in table {
        match item.as_table() {
            Some(section) => {
                for (inner, inner_item) in section {
                    out.push((format!("{name}.{inner}"), inner_item));
                }
            }
            None => out.push((name.to_owned(), item)),
        }
    }
    out
}

/// Every setting with its value and origin, after all layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    values: BTreeMap<&'static str, (Value, Origin)>,
}

/// What to resolve from. `env` looks up environment variables; `home` is
/// `$HOME`, for `~/` in paths.
pub struct Sources<'a> {
    pub global: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub flags: Vec<(&'static str, String)>,
}

impl Resolved {
    pub fn resolve(sources: &Sources) -> Result<Self, ConfigError> {
        let env = sources.env;
        let home = env("HOME");
        let home = home.as_deref();
        let mut values: BTreeMap<&'static str, (Value, Origin)> = BTreeMap::new();
        for (name, value) in defaults(env) {
            values.insert(name, (value, Origin::Default));
        }
        let files = [
            sources.global.clone().map(Origin::Global),
            sources.repo.clone().map(Origin::Repo),
        ];
        for origin in files.into_iter().flatten() {
            let path = match &origin {
                Origin::Global(p) | Origin::Repo(p) => p.clone(),
                _ => unreachable!(),
            };
            for (key, value) in read_file(&path, &origin, home)? {
                values.insert(key.name, (value, origin.clone()));
            }
        }
        for key in KEYS {
            let Some(var) = key.env else { continue };
            let Some(text) = env(var).filter(|t| !t.is_empty()) else {
                continue;
            };
            let value = parse(key, &text, home).map_err(|problem| ConfigError::InvalidValue {
                key: key.name.to_owned(),
                location: Some(format!("environment variable {var}")),
                problem,
            })?;
            values.insert(key.name, (value, Origin::Env(var)));
        }
        for (name, text) in &sources.flags {
            let key = key(name)?;
            let value = parse(key, text, home).map_err(|problem| ConfigError::InvalidValue {
                key: key.name.to_owned(),
                location: Some("the command line".to_owned()),
                problem,
            })?;
            values.insert(key.name, (value, Origin::Flag));
        }
        Ok(Self { values })
    }

    /// Value and origin of `name`, if it has a value (paths need `$HOME`
    /// for their defaults).
    pub fn get(&self, name: &str) -> Option<&(Value, Origin)> {
        self.values.get(name)
    }

    /// Every key in [`KEYS`] order.
    pub fn entries(&self) -> impl Iterator<Item = (&'static Key, Option<&(Value, Origin)>)> {
        KEYS.iter().map(|k| (k, self.values.get(k.name)))
    }

    fn text(&self, name: &str) -> Option<&str> {
        match self.values.get(name) {
            Some((Value::Text(t), _)) => Some(t),
            _ => None,
        }
    }

    fn count(&self, name: &str) -> usize {
        match self.values.get(name) {
            Some((Value::Count(n), _)) => *n,
            _ => unreachable!("{name} has a default"),
        }
    }

    pub fn settings(&self) -> Settings {
        let choice = |name| self.text(name).expect("choices have defaults");
        Settings {
            backend: match choice("backend") {
                "openvino" => BackendKind::Openvino,
                "llamacpp" => BackendKind::LlamaCpp,
                "mock" => BackendKind::Mock,
                _ => BackendKind::Auto,
            },
            device: match choice("model.device") {
                "npu" => DeviceSetting::Npu,
                "gpu" => DeviceSetting::Gpu,
                "cpu" => DeviceSetting::Cpu,
                "vulkan" => DeviceSetting::Vulkan,
                _ => DeviceSetting::Auto,
            },
            model_dir: self.text("model.dir").map(PathBuf::from),
            gguf: self.text("model.gguf").map(PathBuf::from),
            llama_server: self.text("model.llama_server").map(PathBuf::from),
            vulkan_device: self.count("model.vulkan_device"),
            npu: NpuTuning {
                prefill_chunk: self.count("model.npu_prefill_chunk"),
                best_perf: choice("model.npu_generate_hint") == "best_perf",
            },
            cache_dir: self.text("model.cache_dir").map(PathBuf::from),
            max_prompt_tokens: self.count("model.max_prompt_tokens"),
            max_new_tokens: self.count("model.max_new_tokens"),
            convention: convention(choice("style.convention")),
            language: language(choice("style.language")),
            default_convention: convention(choice("style.default_convention")).expect("not auto"),
            default_language: language(choice("style.default_language")).expect("not auto"),
            history_depth: self.count("style.history_depth"),
        }
    }
}

fn convention(text: &str) -> Option<Convention> {
    match text {
        "conventional" => Some(Convention::Conventional),
        "gitmoji" => Some(Convention::Gitmoji),
        "free" => Some(Convention::Free),
        _ => None,
    }
}

fn language(text: &str) -> Option<Language> {
    match text {
        "english" => Some(Language::English),
        "portuguese" => Some(Language::Portuguese),
        _ => None,
    }
}

/// Built-in values. Paths have no default without `$HOME`.
fn defaults(env: &dyn Fn(&str) -> Option<String>) -> Vec<(&'static str, Value)> {
    let text = |s: &str| Value::Text(s.to_owned());
    let path = |p: Option<PathBuf>| p.map(|p| Value::Text(p.to_string_lossy().into_owned()));
    let mut values = vec![
        ("backend", text("auto")),
        ("model.device", text("auto")),
        ("model.vulkan_device", Value::Count(0)),
        (
            "model.npu_prefill_chunk",
            Value::Count(NpuTuning::default().prefill_chunk),
        ),
        ("model.npu_generate_hint", text("best_perf")),
        (
            "model.max_prompt_tokens",
            Value::Count(DEFAULT_MAX_PROMPT_TOKENS),
        ),
        (
            "model.max_new_tokens",
            Value::Count(GenerationConfig::default().max_new_tokens),
        ),
        ("style.convention", text("auto")),
        ("style.language", text("auto")),
        ("style.default_convention", text("conventional")),
        ("style.default_language", text("english")),
        (
            "style.history_depth",
            Value::Count(crate::context::HISTORY_DEPTH),
        ),
    ];
    if let Some(dir) = path(paths::default_model_dir(env)) {
        values.push(("model.dir", dir));
    }
    if let Some(dir) = path(paths::default_cache_dir(env)) {
        values.push(("model.cache_dir", dir));
    }
    if let Some(file) = path(paths::default_gguf_file(env)) {
        values.push(("model.gguf", file));
    }
    if let Some(file) = path(paths::default_llama_server(env)) {
        values.push(("model.llama_server", file));
    }
    values
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Auto,
    Openvino,
    LlamaCpp,
    Mock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSetting {
    Auto,
    Npu,
    Gpu,
    Cpu,
    Vulkan,
}

/// Every setting, typed and resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub backend: BackendKind,
    pub device: DeviceSetting,
    pub model_dir: Option<PathBuf>,
    pub gguf: Option<PathBuf>,
    pub llama_server: Option<PathBuf>,
    pub vulkan_device: usize,
    pub npu: NpuTuning,
    pub cache_dir: Option<PathBuf>,
    pub max_prompt_tokens: usize,
    pub max_new_tokens: usize,
    pub convention: Option<Convention>,
    pub language: Option<Language>,
    pub default_convention: Convention,
    pub default_language: Language,
    pub history_depth: usize,
}

impl Settings {
    pub fn prompt_options(&self) -> PromptOptions {
        PromptOptions {
            max_prompt_tokens: self.max_prompt_tokens,
            default_convention: self.default_convention,
            default_language: self.default_language,
            convention: self.convention,
            language: self.language,
        }
    }

    pub fn generation_config(&self) -> GenerationConfig {
        GenerationConfig {
            max_new_tokens: self.max_new_tokens,
        }
    }

    pub fn genai_options(&self, device: Device) -> Result<GenaiOptions, String> {
        let missing = |key: &str| format!("{key} has no default without $HOME; set it");
        let chunk = self.npu.prefill_chunk;
        if device == Device::Npu
            && chunk < self.max_prompt_tokens
            && !self.max_prompt_tokens.is_multiple_of(chunk)
        {
            return Err(format!(
                "model.max_prompt_tokens ({}) must be a multiple of model.npu_prefill_chunk \
                 ({chunk}) on the NPU",
                self.max_prompt_tokens
            ));
        }
        Ok(GenaiOptions {
            model_dir: self.model_dir.clone().ok_or_else(|| missing("model.dir"))?,
            device,
            cache_dir: self
                .cache_dir
                .clone()
                .ok_or_else(|| missing("model.cache_dir"))?,
            // The NPU compiles for exactly the prompt length we budget for.
            max_prompt_tokens: self.max_prompt_tokens,
            max_new_tokens: self.max_new_tokens,
            npu: self.npu,
        })
    }

    pub fn llamacpp_options(&self) -> Result<LlamaCppOptions, String> {
        let missing = |key: &str| format!("{key} has no default without $HOME; set it");
        Ok(LlamaCppOptions {
            server: self
                .llama_server
                .clone()
                .ok_or_else(|| missing("model.llama_server"))?,
            model: self.gguf.clone().ok_or_else(|| missing("model.gguf"))?,
            vulkan_device: self.vulkan_device,
            max_prompt_tokens: self.max_prompt_tokens,
            max_new_tokens: self.max_new_tokens,
            startup_timeout: std::time::Duration::from_secs(120),
        })
    }
}

/// Sets `name` to `text` in the file at `path`, keeping its comments and
/// layout; creates the file if needed.
pub fn set_in_file(
    path: &Path,
    origin: &Origin,
    name: &str,
    text: &str,
    home: Option<&str>,
) -> Result<Value, ConfigError> {
    let key = key(name)?;
    if key.scope == Scope::User && matches!(origin, Origin::Repo(_)) {
        return Err(ConfigError::UserOnly {
            key: name.to_owned(),
            path: path.display().to_string(),
        });
    }
    let value = parse(key, text, home).map_err(|problem| ConfigError::InvalidValue {
        key: name.to_owned(),
        location: None,
        problem,
    })?;
    let mut doc = load_document(path)?;
    let item = match &value {
        Value::Text(t) => toml_edit::value(t.as_str()),
        Value::Count(n) => toml_edit::value(i64::try_from(*n).expect("small counts")),
    };
    // A file from `config init` has the key commented out under its
    // description: uncommenting that line keeps the two together.
    if !has_key(&doc, name)
        && let Some(text) = uncomment_template_line(&doc.to_string(), name, &item)
        && let Ok(edited) = text.parse::<DocumentMut>()
        && has_key(&edited, name)
    {
        save_document(path, &edited)?;
        return Ok(value);
    }
    match name.split_once('.') {
        Some((section, field)) => {
            let table = doc
                .entry(section)
                .or_insert_with(|| Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| ConfigError::InvalidValue {
                    key: section.to_owned(),
                    location: Some(path.display().to_string()),
                    problem: "expected a table".to_owned(),
                })?;
            table[field] = item;
        }
        None => doc[name] = item,
    }
    save_document(path, &doc)?;
    Ok(value)
}

fn has_key(doc: &DocumentMut, name: &str) -> bool {
    match name.split_once('.') {
        Some((section, field)) => doc
            .get(section)
            .and_then(Item::as_table)
            .is_some_and(|t| t.contains_key(field)),
        None => doc.contains_key(name),
    }
}

/// Replaces the template's `# field = default` line for `name` (inside its
/// `[section]`, or before any section for top-level keys) with the value.
fn uncomment_template_line(text: &str, name: &str, item: &Item) -> Option<String> {
    let (section, field) = match name.split_once('.') {
        Some((section, field)) => (Some(section), field),
        None => (None, name),
    };
    let commented = format!("# {field} = ");
    let mut current: Option<&str> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(header.trim());
        }
        if !replaced && current == section && line.starts_with(&commented) {
            lines.push(format!("{field} = {}", item.to_string().trim()));
            replaced = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    replaced.then(|| lines.join("\n") + "\n")
}

/// Removes `name` from the file at `path`. Returns whether it was there.
pub fn unset_in_file(path: &Path, name: &str) -> Result<bool, ConfigError> {
    key(name)?;
    if !path.exists() {
        return Ok(false);
    }
    let mut doc = load_document(path)?;
    let removed = match name.split_once('.') {
        Some((section, field)) => doc
            .get_mut(section)
            .and_then(Item::as_table_mut)
            .and_then(|table| table.remove(field))
            .is_some(),
        None => doc.remove(name).is_some(),
    };
    if removed {
        save_document(path, &doc)?;
    }
    Ok(removed)
}

fn load_document(path: &Path) -> Result<DocumentMut, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(ConfigError::Io(format!(
                "cannot read {}: {err}",
                path.display()
            )));
        }
    };
    text.parse()
        .map_err(|err: toml_edit::TomlError| ConfigError::Syntax {
            path: path.display().to_string(),
            problem: err.message().to_owned(),
        })
}

fn save_document(path: &Path, doc: &DocumentMut) -> Result<(), ConfigError> {
    let io =
        |err: std::io::Error| ConfigError::Io(format!("cannot write {}: {err}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, doc.to_string()).map_err(io)
}

/// A file listing every key allowed in `scope`'s file, all commented out,
/// with its description and default.
pub fn template(for_repo: bool, resolved_defaults: &Resolved) -> String {
    let mut out = String::from(if for_repo {
        "# smart-commit settings for this repository, shared through git.\n\
         # They override the global config; flags and environment variables\n\
         # override them. Uncomment a line to change it.\n"
    } else {
        "# smart-commit global settings. A repository's .smart-commit.toml overrides\n\
         # the style.* keys; flags and environment variables override both.\n\
         # `smart-commit config list --show-origin` shows where each value comes\n\
         # from. Uncomment a line to change it.\n"
    });
    let mut section = None;
    for key in KEYS {
        if for_repo && key.scope == Scope::User {
            continue;
        }
        let (current, field) = match key.name.split_once('.') {
            Some((s, f)) => (Some(s), f),
            None => (None, key.name),
        };
        if current != section {
            if let Some(s) = current {
                out.push_str(&format!("\n[{s}]\n"));
            }
            section = current;
        }
        out.push('\n');
        for line in wrap(key.description, 74) {
            out.push_str(&format!("# {line}\n"));
        }
        if let Kind::Choice(choices) = key.kind {
            out.push_str(&format!("# One of: {}.\n", choices.join(", ")));
        }
        if let Some(var) = key.env {
            out.push_str(&format!("# Environment: {var}.\n"));
        }
        let default = match resolved_defaults.get(key.name) {
            Some((Value::Text(t), _)) => format!("{t:?}"),
            Some((Value::Count(n), _)) => n.to_string(),
            None => "\"\"".to_owned(),
        };
        out.push_str(&format!("# {field} = {default}\n"));
    }
    out
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().expect("never empty");
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_owned());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    const HOME: &[(&str, &str)] = &[("HOME", "/home/me")];

    fn resolve(
        global: Option<&str>,
        repo: Option<&str>,
        vars: &[(&str, &str)],
        flags: Vec<(&'static str, String)>,
    ) -> Result<Resolved, ConfigError> {
        let dir = tempfile::TempDir::new().unwrap();
        let write = |name: &str, text: Option<&str>| {
            text.map(|t| {
                let path = dir.path().join(name);
                std::fs::write(&path, t).unwrap();
                path
            })
        };
        let lookup = env(vars);
        Resolved::resolve(&Sources {
            global: write("global.toml", global),
            repo: write("repo.toml", repo),
            env: &lookup,
            flags,
        })
    }

    #[test]
    fn defaults_match_the_built_in_options() {
        let settings = resolve(None, None, HOME, vec![]).unwrap().settings();
        assert_eq!(settings.prompt_options(), PromptOptions::default());
        assert_eq!(settings.generation_config(), GenerationConfig::default());
        assert_eq!(settings.backend, BackendKind::Auto);
        assert_eq!(settings.device, DeviceSetting::Auto);
        assert_eq!(settings.gguf, paths::default_gguf_file(env(HOME)));
        assert_eq!(
            settings.llama_server,
            paths::default_llama_server(env(HOME))
        );
        assert_eq!(settings.history_depth, crate::context::HISTORY_DEPTH);
        assert_eq!(settings.model_dir, paths::default_model_dir(env(HOME)));
    }

    #[test]
    fn layers_override_in_order() {
        let global = "backend = \"mock\"\n[model]\ndevice = \"gpu\"\n[style]\nlanguage = \"english\"\nhistory_depth = 10\n";
        let repo = "[style]\nlanguage = \"portuguese\"\n";
        let vars = [("HOME", "/home/me"), ("SMART_COMMIT_DEVICE", "cpu")];
        let resolved = resolve(
            Some(global),
            Some(repo),
            &vars,
            vec![("backend", "openvino".into())],
        )
        .unwrap();
        let origin = |name| resolved.get(name).unwrap().1.clone();
        assert!(matches!(origin("style.history_depth"), Origin::Global(_)));
        assert!(matches!(origin("style.language"), Origin::Repo(_)));
        assert_eq!(origin("model.device"), Origin::Env("SMART_COMMIT_DEVICE"));
        assert_eq!(origin("backend"), Origin::Flag);
        assert_eq!(origin("style.convention"), Origin::Default);

        let settings = resolved.settings();
        assert_eq!(settings.backend, BackendKind::Openvino);
        assert_eq!(settings.device, DeviceSetting::Cpu);
        assert_eq!(settings.language, Some(Language::Portuguese));
        assert_eq!(settings.history_depth, 10);
    }

    #[test]
    fn unknown_keys_are_errors() {
        let err = resolve(Some("[model]\ndevise = \"cpu\"\n"), None, HOME, vec![]).unwrap_err();
        assert!(matches!(&err, ConfigError::UnknownKey { key, .. } if key == "model.devise"));
        assert!(
            err.to_string()
                .contains("known keys: backend, model.device")
        );
        assert!(resolve(Some("[style.extra]\nx = 1\n"), None, HOME, vec![]).is_err());
    }

    #[test]
    fn invalid_values_are_errors() {
        for (text, problem) in [
            (
                "[model]\ndevice = \"tpu\"\n",
                "expected one of auto, npu, gpu, cpu, vulkan, got \"tpu\"",
            ),
            ("[model]\ndevice = 3\n", "expected a string, got 3"),
            (
                "[style]\nhistory_depth = \"30\"\n",
                "expected a whole number, got \"30\"",
            ),
            (
                "[style]\nhistory_depth = 0\n",
                "expected a whole number from 1 to 500, got \"0\"",
            ),
            (
                "[model]\ndir = \"models/here\"\n",
                "expected an absolute path or one starting with ~/",
            ),
            ("model = 1\n", "unknown key `model`"),
        ] {
            let err = resolve(Some(text), None, HOME, vec![]).unwrap_err();
            assert!(err.to_string().contains(problem), "{text:?}: {err}");
        }
        let err = resolve(
            None,
            None,
            &[("HOME", "/h"), ("SMART_COMMIT_BACKEND", "gpt")],
            vec![],
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("environment variable SMART_COMMIT_BACKEND"),
            "{err}"
        );
    }

    #[test]
    fn syntax_errors_name_the_file() {
        let err = resolve(Some("[model\n"), None, HOME, vec![]).unwrap_err();
        assert!(matches!(err, ConfigError::Syntax { path, .. } if path.ends_with("global.toml")));
    }

    #[test]
    fn machine_keys_are_refused_in_the_repository_file() {
        let err = resolve(None, Some("[model]\ndir = \"/tmp/evil\"\n"), HOME, vec![]).unwrap_err();
        assert!(
            matches!(&err, ConfigError::UserOnly { key, .. } if key == "model.dir"),
            "{err}"
        );
        assert!(resolve(None, Some("backend = \"mock\"\n"), HOME, vec![]).is_err());
    }

    #[test]
    fn paths_expand_home() {
        let resolved =
            resolve(Some("[model]\ndir = \"~/models/q\"\n"), None, HOME, vec![]).unwrap();
        assert_eq!(
            resolved.settings().model_dir,
            Some(PathBuf::from("/home/me/models/q"))
        );
    }

    #[test]
    fn empty_environment_variables_are_ignored() {
        let vars = [("HOME", "/h"), ("SMART_COMMIT_DEVICE", "")];
        let resolved = resolve(None, None, &vars, vec![]).unwrap();
        assert_eq!(resolved.get("model.device").unwrap().1, Origin::Default);
    }

    #[test]
    fn without_home_paths_have_no_default() {
        let settings = resolve(None, None, &[], vec![]).unwrap().settings();
        assert_eq!(settings.model_dir, None);
        assert!(
            settings
                .genai_options(Device::Npu)
                .unwrap_err()
                .contains("model.dir")
        );
        assert!(
            settings
                .llamacpp_options()
                .unwrap_err()
                .contains("model.llama_server")
        );
    }

    #[test]
    fn npu_chunk_must_divide_the_prompt_length() {
        let global = "[model]\nmax_prompt_tokens = 1536\nnpu_prefill_chunk = 1024\n";
        let settings = resolve(Some(global), None, HOME, vec![])
            .unwrap()
            .settings();
        let err = settings.genai_options(Device::Npu).unwrap_err();
        assert!(err.contains("must be a multiple"), "{err}");
        // Only the NPU cares; and a chunk at least as large is just static.
        assert!(settings.genai_options(Device::Gpu).is_ok());
        let global = "[model]\nmax_prompt_tokens = 512\nnpu_prefill_chunk = 1024\n";
        let settings = resolve(Some(global), None, HOME, vec![])
            .unwrap()
            .settings();
        assert!(settings.genai_options(Device::Npu).is_ok());
        assert_eq!(settings.npu.prefill_chunk, 1024);
        assert!(settings.npu.best_perf);
    }

    #[test]
    fn every_key_is_documented() {
        let page = include_str!("../docs/configuration.md");
        for key in KEYS {
            assert!(
                page.contains(&format!("| `{}` |", key.name)),
                "{} is missing from docs/configuration.md",
                key.name
            );
            if let Some(var) = key.env {
                assert!(
                    page.contains(var),
                    "{var} is missing from docs/configuration.md"
                );
            }
        }
    }

    #[test]
    fn set_and_unset_keep_comments() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sub/config.toml");
        let origin = Origin::Global(path.clone());
        set_in_file(&path, &origin, "model.device", "cpu", None).unwrap();
        let with_comment = format!("# mine\n{}", std::fs::read_to_string(&path).unwrap());
        std::fs::write(&path, with_comment).unwrap();
        set_in_file(&path, &origin, "style.history_depth", "12", None).unwrap();
        set_in_file(&path, &origin, "backend", "mock", None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        // Top-level keys must precede tables in TOML, so `backend` goes first;
        // the comment stays with the table it was above.
        assert!(text.contains("# mine\n[model]\n"), "{text}");
        let values = parse_file(&text, &path, &origin, None).unwrap();
        let names: Vec<_> = values
            .iter()
            .map(|(k, v)| (k.name, v.to_string()))
            .collect();
        assert!(names.contains(&("model.device", "cpu".into())));
        assert!(names.contains(&("style.history_depth", "12".into())));
        assert!(names.contains(&("backend", "mock".into())));

        assert!(unset_in_file(&path, "model.device").unwrap());
        assert!(!unset_in_file(&path, "model.device").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# mine\n[model]\n") && !text.contains("device"),
            "{text}"
        );
    }

    #[test]
    fn set_uncomments_the_template_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let origin = Origin::Global(path.clone());
        let defaults = resolve(None, None, HOME, vec![]).unwrap();
        std::fs::write(&path, template(false, &defaults)).unwrap();

        set_in_file(&path, &origin, "model.device", "cpu", None).unwrap();
        set_in_file(&path, &origin, "backend", "mock", None).unwrap();
        set_in_file(&path, &origin, "style.history_depth", "12", None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# Environment: SMART_COMMIT_DEVICE.\ndevice = \"cpu\"\n"),
            "{text}"
        );
        assert!(text.contains("# Environment: SMART_COMMIT_BACKEND.\nbackend = \"mock\"\n"));
        assert!(text.contains("history_depth = 12\n"));
        assert!(
            !text.contains("# device = "),
            "the commented line was replaced"
        );
        // `max_prompt_tokens` has its own template line; it must stay put.
        assert!(text.contains("# max_prompt_tokens = 1024"));

        let values = parse_file(&text, &path, &origin, None).unwrap();
        assert_eq!(values.len(), 3);
        // Changing it again edits the now real key.
        set_in_file(&path, &origin, "model.device", "gpu", None).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("SMART_COMMIT_DEVICE.\ndevice = \"gpu\""),
            "{text}"
        );
    }

    #[test]
    fn set_validates_before_writing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let global = Origin::Global(path.clone());
        assert!(set_in_file(&path, &global, "model.device", "tpu", None).is_err());
        assert!(set_in_file(&path, &global, "nope", "1", None).is_err());
        let repo = Origin::Repo(path.clone());
        assert!(matches!(
            set_in_file(&path, &repo, "model.dir", "/m", None),
            Err(ConfigError::UserOnly { .. })
        ));
        assert!(!path.exists(), "nothing was written");
    }

    #[test]
    fn templates_parse_to_the_defaults_and_list_every_key() {
        let defaults = resolve(None, None, HOME, vec![]).unwrap();
        for for_repo in [false, true] {
            let template = template(for_repo, &defaults);
            let path = PathBuf::from("/t.toml");
            let origin = if for_repo {
                Origin::Repo(path.clone())
            } else {
                Origin::Global(path.clone())
            };
            // All commented out: no values.
            assert_eq!(parse_file(&template, &path, &origin, None).unwrap(), vec![]);
            // Uncommented, every line is valid and equals the default.
            let uncommented: String = template
                .lines()
                .map(|l| match l.strip_prefix("# ") {
                    Some(rest) if rest.contains(" = ") && !rest.starts_with(['#', ' ']) => rest,
                    _ => l,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let values = parse_file(&uncommented, &path, &origin, Some("/home/me")).unwrap();
            let expected = KEYS
                .iter()
                .filter(|k| !(for_repo && k.scope == Scope::User))
                .count();
            assert_eq!(values.len(), expected, "{uncommented}");
            for (key, value) in values {
                assert_eq!(&value, &defaults.get(key.name).unwrap().0, "{}", key.name);
            }
        }
    }
}
