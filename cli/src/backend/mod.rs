//! The model that writes the message, behind a trait so everything around
//! it can be tested without an NPU.

pub mod choices;
pub mod device;
pub mod genai;
pub mod llamacpp;
pub mod server;

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::prompt::Prompt;

pub use choices::TypeChoice;

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
    /// Like [`Self::Unavailable`], for a file the backend needs that is not
    /// installed (yet): what `smart-commit setup` fixes.
    #[error("backend unavailable: {0}")]
    NotInstalled(String),
    #[error("generation failed: {0}")]
    Generation(String),
}

/// Timings and token counts of the last generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub load_ms: f32,
    pub time_to_first_token_ms: f32,
    pub time_per_output_token_ms: f32,
    pub generate_ms: f32,
}

pub trait Backend {
    /// Model and device, recorded with each generation.
    fn describe(&self) -> String;

    fn generate(&self, prompt: &Prompt, config: &GenerationConfig) -> Result<String, BackendError>;

    /// The prompt budget, in estimated tokens, this backend can take when
    /// asked for `requested`: less for a backend that fails past a fixed
    /// prompt length, since the estimate can be short (see
    /// [`crate::diff::estimate_tokens`]).
    fn prompt_budget(&self, requested: usize) -> usize {
        requested
    }

    /// The Conventional Commits types the reply to `prompt` may start with,
    /// most likely first, with their probabilities. Empty when the backend
    /// cannot tell (only llama.cpp's devices can).
    fn type_choices(&self, _prompt: &Prompt) -> Result<Vec<TypeChoice>, BackendError> {
        Ok(Vec::new())
    }

    /// Like [`Backend::generate`], with the reply made to begin with
    /// `start`, which is part of what it returns: the type a person chose.
    /// Only a backend whose [`Backend::type_choices`] is not empty has it.
    fn generate_starting_with(
        &self,
        _prompt: &Prompt,
        _start: &str,
        _config: &GenerationConfig,
    ) -> Result<String, BackendError> {
        Err(BackendError::Unavailable(
            "this backend cannot start a reply with a chosen type".to_owned(),
        ))
    }

    /// Timings of the last successful [`Backend::generate`], when the
    /// backend measures them.
    fn last_metrics(&self) -> Option<Metrics> {
        None
    }
}

/// Stands in when no model is needed (see [`crate::generate::by_rules`]),
/// so nothing is loaded; asking it to generate is a bug.
pub struct NoModel;

impl Backend for NoModel {
    fn describe(&self) -> String {
        "none".to_owned()
    }

    fn generate(&self, _: &Prompt, _: &GenerationConfig) -> Result<String, BackendError> {
        Err(BackendError::Unavailable("no model was loaded".to_owned()))
    }
}

/// Replies with scripted responses in order, then with empty ones, and
/// records the prompts it was given.
#[derive(Debug, Default)]
pub struct MockBackend {
    responses: Mutex<VecDeque<String>>,
    prompts: Mutex<Vec<Prompt>>,
    choices: Vec<TypeChoice>,
    starts: Mutex<Vec<String>>,
}

impl MockBackend {
    pub fn new<S: Into<String>>(responses: impl IntoIterator<Item = S>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Into::into).collect()),
            prompts: Mutex::default(),
            choices: Vec::new(),
            starts: Mutex::default(),
        }
    }

    /// Makes it answer [`Backend::type_choices`] with `choices`, and take a
    /// reply start: the scripted replies are then the whole reply.
    pub fn with_type_choices(
        mut self,
        choices: impl IntoIterator<Item = (&'static str, f64)>,
    ) -> Self {
        self.choices = choices
            .into_iter()
            .map(|(name, probability)| TypeChoice {
                name: name.to_owned(),
                probability,
            })
            .collect();
        self
    }

    /// The reply starts it was asked for, oldest first.
    pub fn starts(&self) -> Vec<String> {
        self.starts.lock().expect("mock lock").clone()
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

    fn type_choices(&self, _: &Prompt) -> Result<Vec<TypeChoice>, BackendError> {
        Ok(self.choices.clone())
    }

    /// The scripted reply as it is: it already begins with the start.
    fn generate_starting_with(
        &self,
        prompt: &Prompt,
        start: &str,
        config: &GenerationConfig,
    ) -> Result<String, BackendError> {
        self.starts
            .lock()
            .expect("mock lock")
            .push(start.to_owned());
        self.generate(prompt, config)
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

    #[test]
    fn a_backend_without_choices_says_so_and_cannot_start_a_reply() {
        let mock = MockBackend::new(["fix: x"]);
        assert!(mock.type_choices(&prompt("a")).unwrap().is_empty());
        assert!(NoModel.type_choices(&prompt("a")).unwrap().is_empty());
        let config = GenerationConfig::default();
        let err = NoModel
            .generate_starting_with(&prompt("a"), "fix", &config)
            .unwrap_err();
        assert!(err.to_string().contains("cannot start a reply"), "{err}");
    }

    #[test]
    fn mock_offers_its_choices_and_records_the_starts() {
        let mock = MockBackend::new(["fix(core): stop the crash"])
            .with_type_choices([("feat", 0.55), ("fix", 0.45)]);
        let choices = mock.type_choices(&prompt("a")).unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[1].name, "fix");
        let config = GenerationConfig::default();
        let reply = mock
            .generate_starting_with(&prompt("a"), "fix", &config)
            .unwrap();
        assert_eq!(reply, "fix(core): stop the crash");
        assert_eq!(mock.starts(), ["fix"]);
        assert_eq!(mock.prompts(), [prompt("a")]);
    }
}
