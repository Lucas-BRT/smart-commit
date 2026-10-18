//! Generate commit messages in the style of the current repository using a
//! small LLM running locally on an Intel NPU.

pub mod context;
pub mod diff;
pub mod git;
pub mod message;
pub mod prompt;
pub mod style;
