//! Core types for commit message style inference.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Convention {
    /// No history to learn from.
    Unknown,
    /// <https://www.conventionalcommits.org>: `type(scope)!: description`.
    Conventional,
    /// <https://gitmoji.dev>: subject starts with an emoji or `:shortcode:`.
    Gitmoji,
    Free,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Unknown,
    English,
    Portuguese,
}

/// Whether and how subjects cite an issue key such as `ABC-123`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketStyle {
    None,
    /// The subject, or its description after a convention prefix, starts
    /// with the key: `ABC-123: fix login`, `[ABC-123] fix login`,
    /// `fix: ABC-123 login`.
    Prefix,
    /// The key appears elsewhere in the subject.
    Mention,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub convention: Convention,
    pub language: Language,
    pub tickets: TicketStyle,
    /// Conventional commit types with their counts, most used first.
    pub types: Vec<(String, usize)>,
    /// Conventional commit scopes with their counts, most used first.
    pub scopes: Vec<(String, usize)>,
    /// Recent subjects that represent the style well, most recent first.
    pub examples: Vec<String>,
    /// Number of subjects the style was inferred from.
    pub sample_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionalHeader<'a> {
    pub kind: &'a str,
    pub scope: Option<&'a str>,
    pub breaking: bool,
    pub description: &'a str,
}
