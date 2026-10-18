//! The model that writes the message, behind a trait so everything around
//! it can be tested without an NPU.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::prompt::Prompt;

/// Everything that affects what the model generates, made explicit so a
/// generation can be reproduced: no library defaults are relied upon.
///
/// Decoding is always greedy, so the same prompt, model and config give the
/// same output; there is no seed because nothing is sampled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationConfig {
    /// Enough for a 72-column subject and a short body.
    pub max_new_tokens: usize,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            max_new_tokens: 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    #[error("generation failed: {0}")]
    Generation(String),
}

pub trait Backend {
    /// Model and device, recorded with each generation.
    fn describe(&self) -> String;

    fn generate(&self, prompt: &Prompt, config: &GenerationConfig) -> Result<String, BackendError>;
}

/// Replies with scripted responses in order, then with empty ones, and
/// records the prompts it was given.
#[derive(Debug, Default)]
pub struct MockBackend {
    responses: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<Prompt>>,
}

impl MockBackend {
    pub fn new<S: Into<String>>(responses: impl IntoIterator<Item = S>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Into::into).collect()),
            prompts: Mutex::default(),
        }
    }

    /// The prompts received so far, oldest first.
    pub fn prompts(&self) -> Vec<Prompt> {
        self.prompts.lock().expect("mock lock").clone()
    }
}

impl Backend for MockBackend {
    fn describe(&self) -> String {
        "mock".to_owned()
    }

    fn generate(&self, prompt: &Prompt, _: &GenerationConfig) -> Result<String, BackendError> {
        self.prompts.lock().expect("mock lock").push(prompt.clone());
        Ok(self
            .responses
            .lock()
            .expect("mock lock")
            .pop_front()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(user: &str) -> Prompt {
        Prompt {
            system: "system".into(),
            user: user.into(),
        }
    }

    #[test]
    fn mock_replies_in_order_then_empty() {
        let mock = MockBackend::new(["first", "second"]);
        let config = GenerationConfig::default();
        let replies: Vec<String> = (0..3)
            .map(|n| mock.generate(&prompt(&n.to_string()), &config).unwrap())
            .collect();
        assert_eq!(replies, ["first", "second", ""]);
    }

    #[test]
    fn mock_records_prompts() {
        let mock = MockBackend::default();
        let config = GenerationConfig::default();
        mock.generate(&prompt("a"), &config).unwrap();
        mock.generate(&prompt("b"), &config).unwrap();
        assert_eq!(mock.prompts(), [prompt("a"), prompt("b")]);
        assert_eq!(mock.describe(), "mock");
    }
}
