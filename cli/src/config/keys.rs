//! The settings keys, their types, and the values they accept.

use std::path::{Path, PathBuf};

use toml_edit::Item;

use crate::backend::device;
use crate::models;

use super::{ConfigError, Value};

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
        description: "What runs the model: `openvino` (Intel NPU, Intel GPU, openvino-cpu), \
                      `llamacpp` (any GPU through Vulkan, and the CPU), `auto` (whichever \
                      model.device needs) or `mock`, which replies with --mock-response \
                      (for tests).",
    },
    Key {
        name: "model.device",
        kind: Kind::Choice(&["auto", "npu", "intel-gpu", "vulkan", "cpu", "openvino-cpu"]),
        scope: Scope::User,
        env: Some("SMART_COMMIT_DEVICE"),
        description: "Where the model runs: `npu` (Intel NPU) and `intel-gpu` through OpenVINO, \
                      `vulkan` (AMD, NVIDIA or Intel GPU) and `cpu` (any x86_64) through \
                      llama.cpp, `openvino-cpu` (the CPU through OpenVINO), or `auto`: the \
                      fastest in the last `smart-commit bench`, else the first that works of \
                      intel-gpu, npu, vulkan, cpu, openvino-cpu. `gpu` is the old name of \
                      intel-gpu.",
    },
    Key {
        name: "model.name",
        kind: Kind::Choice(models::IDS),
        scope: Scope::User,
        env: Some("SMART_COMMIT_MODEL"),
        description: "Which recommended model runs (`smart-commit model` lists them), \
                      downloaded from Hugging Face, pinned and checked.",
    },
    Key {
        name: "model.dir",
        kind: Kind::Path,
        scope: Scope::User,
        env: Some("SMART_COMMIT_MODEL_DIR"),
        description: "The OpenVINO model; by default model.name's, exported on this machine \
                      by `smart-commit setup`. Another directory uses that model instead.",
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
        description: "The GGUF model of the llama.cpp devices; by default model.name's. \
                      Another file (`smart-commit model use FILE`) uses that model instead.",
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
        name: "model.fast_load",
        kind: Kind::Choice(&["auto", "on", "off"]),
        scope: Scope::User,
        env: Some("SMART_COMMIT_FAST_LOAD"),
        description: "llama.cpp devices: start llama-server without repacking the weights \
                      and fitting them to GPU memory (`--no-repack --fit off`). `auto` uses \
                      it where the last `smart-commit bench` found it faster.",
    },
    Key {
        name: "model.threads_batch",
        kind: Kind::Count { min: 0, max: 1024 },
        scope: Scope::User,
        env: None,
        description: "llama.cpp on the CPU: threads that read the prompt (`--threads-batch`). \
                      0: one per physical core, which llama.cpp's own default can miss on \
                      hybrid CPUs. Replies do not depend on it.",
    },
    Key {
        name: "model.keep_loaded",
        kind: Kind::Count { min: 0, max: 1440 },
        scope: Scope::User,
        env: Some("SMART_COMMIT_KEEP_LOADED"),
        description: "llama.cpp devices: minutes to keep the model loaded after a message, so \
                      the next commit skips loading it (most of a commit on a GPU), at the \
                      cost of its memory meanwhile. 0: stop it after each command. Replies \
                      do not depend on it.",
    },
    Key {
        name: "model.keep_loaded_max_ram",
        kind: Kind::Count { min: 1, max: 100 },
        scope: Scope::User,
        env: Some("SMART_COMMIT_KEEP_LOADED_MAX_RAM"),
        description: "The share of the machine's RAM, in percent, a model kept loaded may take: \
                      the model file and about 256 MB. A model that would take more is loaded \
                      for the commit only, one kept stops when it passes the share, and a \
                      machine with less than a tenth of its RAM available keeps none. 100 \
                      removes the share (not the tenth that stays available).",
    },
    Key {
        name: "model.ask_type",
        kind: Kind::Choice(&["off", "low_confidence"]),
        scope: Scope::User,
        env: Some("SMART_COMMIT_ASK_TYPE"),
        description: "`smart-commit --commit` in a terminal, on llama.cpp devices: with \
                      `low_confidence`, ask which type the message should have when the model \
                      is unsure of it (see model.ask_type_below), offering its likeliest ones, \
                      and write the message for the one chosen. `off`: never ask.",
    },
    Key {
        name: "model.ask_type_below",
        kind: Kind::Count { min: 1, max: 99 },
        scope: Scope::User,
        env: Some("SMART_COMMIT_ASK_TYPE_BELOW"),
        description: "With model.ask_type on low_confidence: ask when the model's confidence in \
                      the type it would pick is below this percentage. On eval-v1, 50 asks \
                      about a quarter of the commits.",
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
        name: "style.default_convention",
        kind: Kind::Choice(&["conventional", "gitmoji", "free"]),
        scope: Scope::Any,
        env: None,
        description: "Convention when `auto` has no history to learn from.",
    },
    Key {
        name: "style.history_depth",
        kind: Kind::Count { min: 1, max: 500 },
        scope: Scope::Any,
        env: None,
        description: "How many recent commits the style is learned from.",
    },
    Key {
        name: "style.subject_max",
        kind: Kind::Count { min: 40, max: 200 },
        scope: Scope::Any,
        env: None,
        description: "Longest subject, in columns: the prompt asks for it, and longer replies \
                      end at the last phrase that fits.",
    },
    Key {
        name: "update.check",
        kind: Kind::Choice(&["false", "true"]),
        scope: Scope::User,
        env: Some("SMART_COMMIT_UPDATE_CHECK"),
        description: "With `true`, at most once a day, an interactive command asks GitHub \
                      (api.github.com, over HTTPS, with curl) for the latest release and \
                      prints a line on stderr when it is newer than this one. The only \
                      request smart-commit makes by itself; `false`: never.",
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

/// Keys earlier versions had, which a file may still hold: they are read and
/// ignored, so that a configuration written by one of them still works.
/// `style.language` and `style.default_language` asked for Portuguese; the
/// messages are in English now.
pub const RETIRED: &[&str] = &["style.language", "style.default_language"];

/// The names of every key, for the error that reports an unknown one.
pub(super) fn known_keys() -> String {
    KEYS.iter().map(|k| k.name).collect::<Vec<_>>().join(", ")
}

/// Values renamed in a release, still accepted: (key, old, new).
const RENAMED: &[(&str, &str, &str)] = &[("model.device", device::OLD_INTEL_GPU_NAME, "intel-gpu")];

/// The current name of `text`, if it is an old value of `key`.
pub(super) fn renamed(key: &str, text: &str) -> Option<&'static str> {
    RENAMED
        .iter()
        .find(|(k, old, _)| *k == key && *old == text)
        .map(|(_, _, new)| *new)
}

/// Checks a value given as text (environment, flag, `config set`).
pub fn parse(key: &Key, text: &str, home: Option<&str>) -> Result<Value, String> {
    match key.kind {
        Kind::Choice(choices) => {
            // Old names are kept as they are, so warnings can show them.
            if choices.contains(&text) || renamed(key.name, text).is_some() {
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
pub(super) fn parse_item(key: &Key, item: &Item, home: Option<&str>) -> Result<Value, String> {
    match (key.kind, item.as_value()) {
        (Kind::Count { .. }, Some(value)) => match value.as_integer() {
            Some(n) if n >= 0 => parse(key, &n.to_string(), home),
            _ => Err(format!(
                "expected a whole number, got {}",
                value.to_string().trim()
            )),
        },
        // A switch written as a TOML boolean (`check = true`) is the choice
        // of that name, so the file reads the way one would write it.
        (Kind::Choice(choices), Some(value))
            if value
                .as_bool()
                .is_some_and(|on| choices.contains(&on.to_string().as_str())) =>
        {
            parse(key, &value.as_bool().unwrap_or_default().to_string(), home)
        }
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
