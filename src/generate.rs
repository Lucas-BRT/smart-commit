//! From context and diff to a commit message: prompt, generate, sanitize,
//! retry once, and fall back to a message built from the file list.

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::context::RepoContext;
use crate::diff::FileDiff;
use crate::message::{self, CommitMessage, SanitizeError};
use crate::prompt::{self, Prompt, PromptOptions};
use crate::style::Convention;

/// The first try plus one retry that says what was wrong.
pub const MAX_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub prompt: Prompt,
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
    let avoid = avoid_note(avoid);
    let mut attempts: Vec<Attempt> = Vec::new();
    while attempts.len() < MAX_ATTEMPTS {
        let retry = attempts
            .last()
            .and_then(|a| a.rejected.as_ref())
            .map(|error| retry_note(error, convention));
        let note = match (&avoid, retry) {
            (Some(avoid), Some(retry)) => Some(format!("{avoid}\n{retry}")),
            (avoid, retry) => retry.or_else(|| avoid.clone()),
        };
        let prompt = prompt::build_with_note(ctx, files, options, note.as_deref());
        let raw = backend.generate(&prompt, config)?;
        match message::sanitize(&raw, convention) {
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
    Ok(Generation {
        message: message::fallback(&ctx.staged, convention, language),
        source: Source::Fallback,
        attempts,
    })
}

/// Asks for a message other than the subjects of `messages`.
pub fn avoid_note(messages: &[String]) -> Option<String> {
    if messages.is_empty() {
        return None;
    }
    let subjects: Vec<String> = messages
        .iter()
        .map(|m| {
            let subject: String = m
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(message::SUBJECT_MAX)
                .collect();
            format!("\"{subject}\"")
        })
        .collect();
    Some(format!(
        "The user turned down {}: {}. Write a different message that describes \
         the change better.",
        if messages.len() == 1 {
            "this message"
        } else {
            "these messages"
        },
        subjects.join("; ")
    ))
}

/// Tells the model why its previous reply was rejected.
pub fn retry_note(error: &SanitizeError, convention: Convention) -> String {
    let reason = match error {
        SanitizeError::Empty => "it contained no commit message".to_owned(),
        SanitizeError::NotConventional(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            format!("\"{subject}\" does not follow `type(scope): description`")
        }
    };
    let format = match convention {
        Convention::Conventional => " in the `type(scope): description` format",
        _ => "",
    };
    format!(
        "Your previous reply was rejected because {reason}. \
         Reply again with only the commit message{format}."
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::backend::MockBackend;
    use crate::git::{ChangeStatus, FileChange};

    fn context(subjects: &[&str]) -> RepoContext {
        RepoContext::new(
            PathBuf::from("/repo"),
            Some("main".into()),
            subjects.iter().map(|s| s.to_string()).collect(),
            vec![FileChange {
                status: ChangeStatus::Modified,
                path: "src/git.rs".into(),
                old_path: None,
            }],
        )
    }

    fn files() -> Vec<FileDiff> {
        vec![FileDiff {
            path: "src/git.rs".into(),
            old_path: None,
            lines: Some((1, 1)),
            hunks: "@@ -1 +1 @@\n-old\n+new\n".into(),
        }]
    }

    fn run(subjects: &[&str], mock: &MockBackend) -> Generation {
        generate(
            &context(subjects),
            &files(),
            mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
        )
        .unwrap()
    }

    const CONVENTIONAL: &[&str] = &["feat(git): add history", "fix(git): handle detached HEAD"];

    #[test]
    fn first_valid_reply_wins() {
        let mock = MockBackend::new(["```\nfix(git): read the branch name\n```"]);
        let generation = run(CONVENTIONAL, &mock);
        assert_eq!(generation.source, Source::Model);
        assert_eq!(generation.message.subject, "fix(git): read the branch name");
        assert_eq!(generation.attempts.len(), 1);
        assert_eq!(
            generation.attempts[0].raw,
            "```\nfix(git): read the branch name\n```"
        );
        assert_eq!(mock.prompts().len(), 1);
    }

    #[test]
    fn retry_tells_the_model_what_was_wrong() {
        let mock = MockBackend::new(["Read the branch name", "fix(git): read the branch name"]);
        let generation = run(CONVENTIONAL, &mock);
        assert_eq!(generation.source, Source::Model);
        assert_eq!(generation.message.subject, "fix(git): read the branch name");

        let prompts = mock.prompts();
        assert_eq!(prompts.len(), 2);
        assert!(!prompts[0].user.contains("rejected"));
        assert!(prompts[1].user.ends_with(
            "Your previous reply was rejected because \"Read the branch name\" does not \
             follow `type(scope): description`. Reply again with only the commit message \
             in the `type(scope): description` format.\n"
        ));
        assert_eq!(
            generation.attempts[0].rejected,
            Some(SanitizeError::NotConventional(
                "Read the branch name".into()
            ))
        );
        assert_eq!(generation.attempts[1].rejected, None);
    }

    #[test]
    fn falls_back_after_the_last_attempt() {
        let mock = MockBackend::new(["", "<think>hmm"]);
        let generation = run(&["Add history", "Fix the detached HEAD"], &mock);
        assert_eq!(generation.source, Source::Fallback);
        assert_eq!(generation.message.subject, "Update src/git.rs");
        assert_eq!(generation.attempts.len(), MAX_ATTEMPTS);
        assert!(
            generation
                .attempts
                .iter()
                .all(|a| a.rejected == Some(SanitizeError::Empty))
        );
    }

    #[test]
    fn same_inputs_give_the_same_generation() {
        let replies = ["nope", "fix(git): read the branch name"];
        let first = run(CONVENTIONAL, &MockBackend::new(replies));
        let second = run(CONVENTIONAL, &MockBackend::new(replies));
        assert_eq!(first, second);
    }

    struct Failing;

    impl Backend for Failing {
        fn describe(&self) -> String {
            "failing".into()
        }

        fn generate(&self, _: &Prompt, _: &GenerationConfig) -> Result<String, BackendError> {
            Err(BackendError::Generation("device lost".into()))
        }
    }

    #[test]
    fn backend_errors_are_not_hidden_by_the_fallback() {
        let result = generate(
            &context(CONVENTIONAL),
            &files(),
            &Failing,
            &PromptOptions::default(),
            &GenerationConfig::default(),
        );
        assert_eq!(result, Err(BackendError::Generation("device lost".into())));
    }

    #[test]
    fn another_message_is_asked_for_deterministically() {
        assert_eq!(avoid_note(&[]), None);
        assert_eq!(
            avoid_note(&["feat: add a\n\nbody".to_owned()]).unwrap(),
            "The user turned down this message: \"feat: add a\". Write a different \
             message that describes the change better."
        );
        let two = avoid_note(&["feat: a".to_owned(), "fix: b".to_owned()]).unwrap();
        assert!(two.starts_with("The user turned down these messages: \"feat: a\"; \"fix: b\"."));

        let ctx = context(&["feat: add login"]);
        let files = files();
        let backend = MockBackend::new(["feat: add retry"]);
        let avoid = vec!["feat: add loop".to_owned()];
        let generation = generate_avoiding(
            &ctx,
            &files,
            &backend,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &avoid,
        )
        .unwrap();
        assert_eq!(generation.message.subject, "feat: add retry");
        let user = &generation.attempts[0].prompt.user;
        assert!(user.contains(&avoid_note(&avoid).unwrap()), "{user}");
        let again = generate_avoiding(
            &ctx,
            &files,
            &MockBackend::new(["feat: add retry"]),
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &avoid,
        )
        .unwrap();
        assert_eq!(again.attempts[0].prompt, generation.attempts[0].prompt);
    }

    #[test]
    fn a_retry_keeps_asking_for_another_message() {
        let ctx = context(&["feat: add login", "fix: typo"]);
        let backend = MockBackend::new(["", "feat: add retry"]);
        let avoid = vec!["feat: add loop".to_owned()];
        let generation = generate_avoiding(
            &ctx,
            &files(),
            &backend,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &avoid,
        )
        .unwrap();
        let user = &generation.attempts[1].prompt.user;
        assert!(user.contains("The user turned down this message"), "{user}");
        assert!(user.contains("Your previous reply was rejected"), "{user}");
    }

    #[test]
    fn retry_note_without_a_required_format() {
        assert_eq!(
            retry_note(&SanitizeError::Empty, Convention::Free),
            "Your previous reply was rejected because it contained no commit message. \
             Reply again with only the commit message."
        );
    }
}
