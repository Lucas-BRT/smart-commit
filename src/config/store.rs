//! The configuration files and the layers they form, resolved into typed
//! settings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use toml_edit::{DocumentMut, Item, Table};

use crate::backend::GenerationConfig;
use crate::backend::device::Target;
use crate::backend::genai::{Device, GenaiOptions, NpuTuning};
use crate::backend::llamacpp::{LlamaCppOptions, LlamaDevice};
use crate::models::{self, Model};
use crate::paths;
use crate::prompt::{
    DEFAULT_MAX_PROMPT_TOKENS, DiffFormat, PromptExamples, PromptOptions, PromptRules, PromptScopes,
};
use crate::style::{Convention, Language};

use super::keys::{KEYS, Key, Scope, key, parse, parse_item, renamed};
use super::{ConfigError, Origin, Value};

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
        // The model files default to those of model.name, whichever layer
        // set it.
        let model = match values.get("model.name") {
            Some((Value::Text(id), _)) => models::find(id),
            _ => None,
        }
        .expect("model.name is checked against the registry");
        if let Some(dir) = paths::default_models_dir(env) {
            let path = |p: PathBuf| {
                (
                    Value::Text(p.to_string_lossy().into_owned()),
                    Origin::Default,
                )
            };
            values
                .entry("model.gguf")
                .or_insert_with(|| path(dir.join(model.gguf.file)));
            if let Some(name) = model.openvino {
                values
                    .entry("model.dir")
                    .or_insert_with(|| path(dir.join(name)));
            }
        }
        Ok(Self { values })
    }

    /// A warning for each value that has an old name, with where it is set.
    pub fn renamed_values(&self) -> Vec<String> {
        self.values
            .iter()
            .filter_map(|(name, (value, origin))| match value {
                Value::Text(text) => renamed(name, text).map(|new| {
                    format!(
                        "{name} = {text} ({origin}) is the old name of {new}; \
                         `smart-commit config set {name} {new}` renames it"
                    )
                }),
                Value::Count(_) => None,
            })
            .collect()
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
            device: Target::from_name(choice("model.device"))
                .map_or(DeviceSetting::Auto, DeviceSetting::Fixed),
            model: models::find(choice("model.name")).expect("checked against the registry"),
            model_dir: self.text("model.dir").map(PathBuf::from),
            gguf: self.text("model.gguf").map(PathBuf::from),
            llama_server: self.text("model.llama_server").map(PathBuf::from),
            vulkan_device: self.count("model.vulkan_device"),
            threads_batch: self.count("model.threads_batch"),
            keep_loaded: Duration::from_secs(60 * self.count("model.keep_loaded") as u64),
            keep_loaded_explicit: matches!(
                self.values.get("model.keep_loaded"),
                Some((_, origin)) if *origin != Origin::Default
            ),
            keep_loaded_max_ram: self.count("model.keep_loaded_max_ram"),
            ask_type: match choice("model.ask_type") {
                "low_confidence" => AskType::LowConfidence,
                _ => AskType::Off,
            },
            ask_type_below: self.count("model.ask_type_below"),
            fast_load: match choice("model.fast_load") {
                "on" => FastLoad::On,
                "off" => FastLoad::Off,
                _ => FastLoad::Auto,
            },
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
            subject_max: self.count("style.subject_max"),
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
        ("model.name", text(models::DEFAULT)),
        ("model.vulkan_device", Value::Count(0)),
        ("model.fast_load", text("auto")),
        ("model.keep_loaded_max_ram", Value::Count(15)),
        ("model.ask_type", text("off")),
        ("model.ask_type_below", Value::Count(50)),
        ("model.threads_batch", Value::Count(0)),
        ("model.keep_loaded", Value::Count(10)),
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
        (
            "style.subject_max",
            Value::Count(crate::message::SUBJECT_MAX),
        ),
    ];
    if let Some(dir) = path(paths::default_cache_dir(env)) {
        values.push(("model.cache_dir", dir));
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
    Fixed(Target),
}

/// `model.ask_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskType {
    Off,
    /// When the model is unsure of the type.
    LowConfidence,
}

/// `model.fast_load`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastLoad {
    /// Where the last benchmark found it faster.
    Auto,
    On,
    Off,
}

impl FastLoad {
    /// Whether to use it, asking `measured_faster` only for `auto`.
    pub fn enabled(self, measured_faster: impl FnOnce() -> bool) -> bool {
        match self {
            Self::Auto => measured_faster(),
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// Every setting, typed and resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub backend: BackendKind,
    pub device: DeviceSetting,
    pub model: &'static Model,
    pub model_dir: Option<PathBuf>,
    pub gguf: Option<PathBuf>,
    pub llama_server: Option<PathBuf>,
    pub vulkan_device: usize,
    /// `model.threads_batch`; 0 for one per physical core.
    pub threads_batch: usize,
    /// `model.keep_loaded`; zero to stop the server after each command.
    pub keep_loaded: Duration,
    /// Whether `model.keep_loaded` was set by someone (a file, the
    /// environment or a flag) and not left at its default: only they are told
    /// when a model is not kept.
    pub keep_loaded_explicit: bool,
    /// `model.keep_loaded_max_ram`, a percentage of the RAM.
    pub keep_loaded_max_ram: usize,
    pub ask_type: AskType,
    /// `model.ask_type_below`, a percentage.
    pub ask_type_below: usize,
    pub fast_load: FastLoad,
    pub npu: NpuTuning,
    pub cache_dir: Option<PathBuf>,
    pub max_prompt_tokens: usize,
    pub max_new_tokens: usize,
    pub convention: Option<Convention>,
    pub language: Option<Language>,
    pub default_convention: Convention,
    pub default_language: Language,
    pub history_depth: usize,
    pub subject_max: usize,
}

impl Settings {
    /// The confidence (0 to 1) below which the type is asked, when it is.
    pub fn ask_type_below(&self) -> f64 {
        self.ask_type_below as f64 / 100.0
    }

    pub fn prompt_options(&self) -> PromptOptions {
        PromptOptions {
            max_prompt_tokens: self.max_prompt_tokens,
            default_convention: self.default_convention,
            default_language: self.default_language,
            convention: self.convention,
            language: self.language,
            // The rules follow the model file that runs, never a setting.
            rules: self
                .gguf
                .as_deref()
                .map_or(PromptRules::Full, models::prompt_rules),
            // No model in use was trained on the map (training H4b).
            scopes: PromptScopes::List,
            // Only for evaluation (training H14).
            examples: PromptExamples::Recent,
            subject_max: self.subject_max,
            // Measured on eval-v1 with the v0.2 fine-tunes and the base
            // models: better types for all four (training H11).
            diff: DiffFormat::Annotated,
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
        let model_dir = match (&self.model_dir, self.model.openvino) {
            (Some(dir), _) => dir.clone(),
            (None, None) => {
                return Err(format!(
                    "{} has no OpenVINO version, which {} needs; `smart-commit model use {}` \
                     has one",
                    self.model.id,
                    device.target().name(),
                    models::DEFAULT
                ));
            }
            (None, Some(_)) => return Err(missing("model.dir")),
        };
        Ok(GenaiOptions {
            model_dir,
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

    /// For [`Target::Cpu`] or [`Target::Vulkan`] (model.vulkan_device), with
    /// or without [`LlamaCppOptions::fast_load`].
    pub fn llamacpp_options(
        &self,
        target: Target,
        fast_load: bool,
    ) -> Result<LlamaCppOptions, String> {
        let missing = |key: &str| format!("{key} has no default without $HOME; set it");
        Ok(LlamaCppOptions {
            server: self
                .llama_server
                .clone()
                .ok_or_else(|| missing("model.llama_server"))?,
            model: self.gguf.clone().ok_or_else(|| missing("model.gguf"))?,
            device: match target {
                Target::Cpu => LlamaDevice::Cpu,
                _ => LlamaDevice::Vulkan(self.vulkan_device),
            },
            max_prompt_tokens: self.max_prompt_tokens,
            max_new_tokens: self.max_new_tokens,
            threads_batch: match self.threads_batch {
                0 => crate::hardware::physical_cores(Path::new("/sys"))
                    .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from)),
                n => n,
            },
            startup_timeout: std::time::Duration::from_secs(120),
            fast_load,
        })
    }
}
