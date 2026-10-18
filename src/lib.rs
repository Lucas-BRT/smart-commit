//! Generate commit messages in the style of the current repository using a
//! small LLM running locally on an Intel NPU.

pub mod backend;
pub mod config;
pub mod context;
pub mod diff;
pub mod genai;
pub mod generate;
pub mod git;
pub mod hook;
pub mod message;
pub mod paths;
pub mod prompt;
pub mod style;
pub mod trace;
