//! Generate commit messages in the style of the current repository using a
//! small LLM running locally on an Intel NPU.

pub mod backend;
pub mod bench;
pub mod config;
pub mod context;
pub mod diff;
pub mod generate;
pub mod git;
pub mod hardware;
pub mod hook;
pub mod memory;
pub mod message;
pub mod models;
pub mod paths;
pub mod prompt;
pub mod setup;
pub mod style;
pub mod trace;
pub mod tui;
pub mod update_notice;
