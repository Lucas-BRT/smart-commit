//! From context and diff to a commit message: prompt, generate, sanitize,
//! retry once, and fall back to a message built from the file list. When
//! only lockfiles and generated files are staged, the file list says it all
//! and the model is not asked.

mod notes;
mod retry;
mod rules;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::context::RepoContext;
use crate::diff::FileDiff;
use crate::message::{self, CommitMessage, SanitizeError};
use crate::prompt::{self, PromptOptions};
use crate::style::Language;

pub use notes::{avoid_note, retry_note};
pub use retry::{copied_from, in_other_language, prepared_message};
pub use rules::by_rules;

/// The first try plus one retry that says what was wrong.
pub const MAX_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub prompt: crate::prompt::Prompt,
    /// What the model returned, unmodified.
    pub raw: String,
    /// Why the output was rejected, if it was.
    pub rejected: Option<SanitizeError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Model,
    /// Every attempt was rejected; built from the file list.
    Fallback,
    /// Only lockfiles and generated files were staged: built from the file
    /// list without asking the model, whose prompt would not show them.
    Rules,
    /// Git prepared it for the merge, revert or cherry-pick in progress,
    /// which the diff does not show: used as is, as `git commit` would.
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation {
    pub message: CommitMessage,
    pub source: Source,
    pub attempts: Vec<Attempt>,
}

pub fn generate(
    ctx: &RepoContext,
    files: &[FileDiff],
    backend: &dyn Backend,
    options: &PromptOptions,
    config: &GenerationConfig,
) -> Result<Generation, BackendError> {
    generate_avoiding(ctx, files, backend, options, config, &[])
}

/// Like [`generate`], asking for a message other than those in `avoid`
/// (messages the user turned down). Decoding is greedy, so only a different
/// prompt can give a different message; this one depends on `avoid` alone.
pub fn generate_avoiding(
    ctx: &RepoContext,
    files: &[FileDiff],
    backend: &dyn Backend,
    options: &PromptOptions,
    config: &GenerationConfig,
    avoid: &[String],
) -> Result<Generation, BackendError> {
    let (convention, language) = prompt::effective_style(&ctx.style, options);
    // Turned down, git's or the file list's message goes to the model for
    // another.
    if let (true, Some(prepared)) = (avoid.is_empty(), &ctx.prepared) {
        return Ok(Generation {
            message: retry::prepared_message(&prepared.message),
            source: Source::Git,
            attempts: Vec::new(),
        });
    }
    if avoid.is_empty() && rules::by_rules(ctx, files) {
        return Ok(Generation {
            message: message::fallback_after(
                &ctx.staged,
                convention,
                language,
                None,
                options.subject_max,
            ),
            source: Source::Rules,
            attempts: Vec::new(),
        });
    }
    let turned_down: Vec<String> = avoid
        .iter()
        .map(|m| m.lines().next().unwrap_or_default().to_owned())
        .collect();
    let avoid_note_val = notes::avoid_note(avoid);
    let mut attempts: Vec<Attempt> = Vec::new();
    while attempts.len() < MAX_ATTEMPTS {
        let retry = attempts
            .last()
            .and_then(|a| a.rejected.as_ref())
            .map(|error| notes::retry_note(error, convention, language));
        let note = match (&avoid_note_val, retry) {
            (Some(avoid), Some(retry)) => Some(format!("{avoid}\n{retry}")),
            (avoid, retry) => retry.or_else(|| avoid.clone()),
        };
        let prompt = prompt::build_with_note(ctx, files, options, note.as_deref());
        let raw = backend.generate(&prompt, config)?;
        let checked =
            message::sanitize_within(&raw, convention, options.subject_max).and_then(|message| {
                if let Some(subject) = retry::copied_from(&message.subject, &ctx.subjects) {
                    return Err(SanitizeError::CopiedHistory(subject.to_owned()));
                }
                if let Some(subject) = retry::copied_from(&message.subject, &turned_down) {
                    return Err(SanitizeError::TurnedDown(subject.to_owned()));
                }
                // A default language, with no history to tell, is not enforced.
                let known = options.language.is_some() || ctx.style.language != Language::Unknown;
                if known && retry::in_other_language(&message.subject, language, &ctx.subjects) {
                    return Err(SanitizeError::WrongLanguage(message.subject));
                }
                Ok(message)
            });
        match checked {
            Ok(message) => {
                attempts.push(Attempt {
                    prompt,
                    raw,
                    rejected: None,
                });
                return Ok(Generation {
                    message,
                    source: Source::Model,
                    attempts,
                });
            }
            Err(error) => attempts.push(Attempt {
                prompt,
                raw,
                rejected: Some(error),
            }),
        }
    }
    // The last reply rejected for what it said, not its form, still has a
    // type worth keeping.
    let reply = attempts.iter().rev().find_map(|a| match a.rejected {
        Some(
            SanitizeError::CopiedHistory(_)
            | SanitizeError::WrongLanguage(_)
            | SanitizeError::TurnedDown(_),
        ) => message::sanitize_within(&a.raw, convention, options.subject_max)
            .ok()
            .map(|m| m.subject),
        _ => None,
    });
    Ok(Generation {
        message: message::fallback_after(
            &ctx.staged,
            convention,
            language,
            reply.as_deref(),
            options.subject_max,
        ),
        source: Source::Fallback,
        attempts,
    })
}

/// Whether the message for `avoid` comes without a model: from git, or
/// from the file list.
pub fn needs_model(ctx: &RepoContext, files: &[FileDiff], avoid: &[String]) -> bool {
    !avoid.is_empty() || (ctx.prepared.is_none() && !rules::by_rules(ctx, files))
}
