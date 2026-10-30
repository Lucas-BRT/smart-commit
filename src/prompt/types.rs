//! Types and constants for prompt construction.

use serde::{Deserialize, Serialize};

use crate::message;
use crate::style::{Convention, Language};

/// The NPU pipeline's default `MAX_PROMPT_LEN`.
pub const DEFAULT_MAX_PROMPT_TOKENS: usize = 1024;
/// Room for the chat template's role markers around our two messages.
pub(super) const TEMPLATE_OVERHEAD_TOKENS: usize = 40;
/// Types and scopes beyond these are rarely used and only cost tokens.
pub(super) const MAX_LISTED: usize = 8;
/// Places shown per scope by [`PromptScopes::Map`]: enough to tell scopes
/// apart.
pub(super) const MAX_AREAS: usize = 3;
/// The longest map, in characters (about 250 tokens): the server reserves
/// room for a prompt a half longer than the budget, and a monorepo's deep
/// paths would use more. Beyond it the map shows fewer places per scope.
pub(super) const MAX_MAP_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// How much of the fixed instructions the system message carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptRules {
    /// Every rule, for models that were not trained on smart-commit's task.
    #[default]
    Full,
    /// Only what varies by repository (types, scopes, examples) and what
    /// training does not teach (a ticket, Portuguese, gitmoji), for a model
    /// trained to follow the rest without being told: a shorter prompt is
    /// read faster. Never for an untrained model, which would lose the
    /// format (training H6).
    Omitted,
}

impl PromptRules {
    pub fn is_full(&self) -> bool {
        *self == Self::Full
    }
}

/// Whether the prompt lists the names the change declares and the comments it adds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptNames {
    /// Only the file list and the diff.
    #[default]
    Off,
    /// A line of the functions, classes and the like the change declares and one of the comments it adds, read from
    /// all the files and not only the hunks that fit (training H16). It is added to the prompt: the diff keeps the
    /// room it has without it.
    On,
}

impl PromptNames {
    pub fn is_off(&self) -> bool {
        *self == Self::Off
    }
}

/// Which of the repository's subjects the prompt shows as examples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptExamples {
    /// The most recent ones.
    #[default]
    Recent,
    /// The commits that touched the files closest to the staged ones, then
    /// the most recent, to evaluate whether similar changes teach the style
    /// better (training H14): the same number of examples, so the prompt
    /// keeps its size.
    Similar,
    /// As `Similar` with at most three, and the rest the most recent.
    Mixed,
}

impl PromptExamples {
    pub fn is_recent(&self) -> bool {
        *self == Self::Recent
    }
}

/// How the scopes the repository uses are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptScopes {
    /// Their names, most used first.
    #[default]
    List,
    /// Each with the places its commits touched (`cli (src/cli/)`), for a
    /// model trained on that (training H4b): alone it did not help the
    /// models (H4). The map is added to the prompt: the diff keeps the room
    /// it has with the list.
    Map,
}

impl PromptScopes {
    pub fn is_list(&self) -> bool {
        *self == Self::List
    }
}

/// How the file list describes each file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffFormat {
    /// `path | +3 -1`, what the v0.2 fine-tunes were trained on.
    Plain,
    /// With each file's status, renames as `dir/{old => new}`, and
    /// whitespace-only and comment-only changes named (see `annotate`).
    #[default]
    Annotated,
}

impl DiffFormat {
    pub fn is_plain(&self) -> bool {
        *self == Self::Plain
    }

    /// What a trace without the field used: the plain list, the only one
    /// before the annotated list existed.
    fn recorded() -> Self {
        Self::Plain
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptOptions {
    pub max_prompt_tokens: usize,
    /// Used when the history is too short or mixed to tell.
    pub default_convention: Convention,
    pub default_language: Language,
    /// Forces a convention instead of the detected one. Left out of traces
    /// when unset, so older traces still read the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convention: Option<Convention>,
    /// Forces a language instead of the detected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// Left out of traces when full, so older traces still read the same.
    #[serde(default, skip_serializing_if = "PromptRules::is_full")]
    pub rules: PromptRules,
    /// Left out of traces when a list, so older traces still read the same.
    #[serde(default, skip_serializing_if = "PromptScopes::is_list")]
    pub scopes: PromptScopes,
    /// Left out of traces when recent, so older traces still read the same.
    #[serde(default, skip_serializing_if = "PromptExamples::is_recent")]
    pub examples: PromptExamples,
    /// Left out of traces when off, so older traces still read the same.
    #[serde(default, skip_serializing_if = "PromptNames::is_off")]
    pub names: PromptNames,
    /// The longest subject, in columns (`style.subject_max`). Left out of
    /// traces when 72, so older traces, which have none, read the same.
    #[serde(
        default = "default_subject_max",
        skip_serializing_if = "is_default_subject_max"
    )]
    pub subject_max: usize,
    /// Left out of traces when plain, so older traces, which have none,
    /// still read the same.
    #[serde(
        default = "DiffFormat::recorded",
        skip_serializing_if = "DiffFormat::is_plain"
    )]
    pub diff: DiffFormat,
}

impl Default for PromptOptions {
    fn default() -> Self {
        Self {
            max_prompt_tokens: DEFAULT_MAX_PROMPT_TOKENS,
            default_convention: Convention::Conventional,
            default_language: Language::English,
            convention: None,
            language: None,
            rules: PromptRules::Full,
            scopes: PromptScopes::List,
            examples: PromptExamples::Recent,
            names: PromptNames::Off,
            subject_max: message::SUBJECT_MAX,
            diff: DiffFormat::Annotated,
        }
    }
}

fn default_subject_max() -> usize {
    message::SUBJECT_MAX
}

fn is_default_subject_max(max: &usize) -> bool {
    *max == message::SUBJECT_MAX
}
