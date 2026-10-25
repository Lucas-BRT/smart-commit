//! `smart-commit bench --prompts`: times llama-server on prompts printed by
//! `smart-commit prompt`, with llama-server flags of the caller's choice, to
//! see what a setting costs before it is turned into one.

use std::path::Path;
use std::time::Instant;

use crate::backend::llamacpp::{LlamaCppBackend, LlamaCppOptions};
use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::prompt::Prompt;

const SYSTEM_MARK: &str = "===== system =====\n";
const USER_MARK: &str = "===== user =====\n";
const END_MARK: &str = "\n===== ~";

/// A prompt as `smart-commit prompt` prints it: the system message, the user
/// message, and a closing line with the token estimate.
pub fn parse_prompt_file(text: &str) -> Option<Prompt> {
    let after_system = text.split_once(SYSTEM_MARK)?.1;
    let (system, rest) = after_system.split_once(USER_MARK)?;
    let user = rest.split(END_MARK).next()?;
    Some(Prompt {
        system: format!("{}\n", system.trim_end_matches('\n')),
        user: user.to_owned(),
    })
}

/// The `*.txt` files of `dir` as prompts, in name order.
pub fn read_prompts(dir: &Path) -> Result<Vec<(String, Prompt)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    paths.sort();
    let mut prompts = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let prompt = parse_prompt_file(&text).ok_or_else(|| {
            format!(
                "{} is not the output of `smart-commit prompt`",
                path.display()
            )
        })?;
        prompts.push((name.into_owned(), prompt));
    }
    if prompts.is_empty() {
        return Err(format!("no *.txt prompts in {}", dir.display()));
    }
    Ok(prompts)
}

/// What one prompt cost: reading it (prefill) and writing the reply (decode).
#[derive(Debug, Clone, PartialEq)]
pub struct PromptTiming {
    pub name: String,
    pub input_tokens: usize,
    pub prefill_ms: f32,
    pub output_tokens: usize,
    pub decode_ms: f32,
    pub first_line: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerTiming {
    pub load_ms: f32,
    pub prompts: Vec<PromptTiming>,
}

/// Generates for every prompt and reads each one's timings from the backend.
pub fn time_prompts(
    backend: &dyn Backend,
    prompts: &[(String, Prompt)],
    config: &GenerationConfig,
) -> Result<Vec<PromptTiming>, BackendError> {
    prompts
        .iter()
        .map(|(name, prompt)| {
            let reply = backend.generate(prompt, config)?;
            let metrics = backend.last_metrics().ok_or_else(|| {
                BackendError::Generation("the backend reported no timings".to_owned())
            })?;
            Ok(PromptTiming {
                name: name.clone(),
                input_tokens: metrics.input_tokens,
                prefill_ms: metrics.time_to_first_token_ms,
                output_tokens: metrics.output_tokens,
                decode_ms: metrics.generate_ms - metrics.time_to_first_token_ms,
                first_line: reply
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(70)
                    .collect(),
            })
        })
        .collect()
}

/// Starts llama-server with `extra` flags, times `prompts`, stops it.
pub fn time_server(
    options: &LlamaCppOptions,
    extra: &[String],
    prompts: &[(String, Prompt)],
    config: &GenerationConfig,
) -> Result<ServerTiming, BackendError> {
    let started = Instant::now();
    let backend = LlamaCppBackend::start_with(options, None, extra)?;
    let load_ms = started.elapsed().as_secs_f32() * 1000.0;
    let prompts = time_prompts(&backend, prompts, config)?;
    Ok(ServerTiming { load_ms, prompts })
}

impl ServerTiming {
    fn mean(&self, value: impl Fn(&PromptTiming) -> f32) -> f32 {
        self.prompts.iter().map(value).sum::<f32>() / self.prompts.len().max(1) as f32
    }

    /// The mean per message and one line per prompt.
    pub fn render(&self, label: &str) -> String {
        let prefill = self.mean(|p| p.prefill_ms);
        let decode = self.mean(|p| p.decode_ms);
        let mut text = format!(
            "== {label}: load {:.0} ms, prefill {prefill:.0} ms, decode {decode:.0} ms, total/msg {:.0} ms\n",
            self.load_ms,
            self.load_ms + prefill + decode
        );
        for p in &self.prompts {
            text.push_str(&format!(
                "   {:14} in={:4} {:6.0}ms out={:3} {:6.0}ms  {}\n",
                p.name, p.input_tokens, p.prefill_ms, p.output_tokens, p.decode_ms, p.first_line
            ));
        }
        text
    }
}
