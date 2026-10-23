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

mod cli;
mod keys;
mod store;

use std::fmt;
use std::path::PathBuf;

use self::keys::known_keys;

pub use cli::{set_aside_invalid, set_in_file, template, unset_in_file};
pub use keys::{KEYS, Key, Kind, Scope, key, parse};
pub use store::{
    BackendKind, DeviceSetting, FastLoad, Resolved, Settings, Sources, parse_file, read_file,
};

/// The per-repository file, at the root of the work tree.
pub const REPO_FILE: &str = ".smart-commit.toml";

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

#[cfg(test)]
mod tests;
