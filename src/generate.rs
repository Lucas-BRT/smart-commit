//! From context and diff to a commit message: prompt, generate, sanitize,
//! retry once, and fall back to a message built from the file list. When
//! only lockfiles and generated files are staged, the file list says it all
//! and the model is not asked.

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::context::RepoContext;
use crate::diff::{self, FileDiff};
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
            message: prepared_message(&prepared.message),
            source: Source::Git,
            attempts: Vec::new(),
        });
    }
    if avoid.is_empty() && by_rules(ctx, files) {
        return Ok(Generation {
            message: message::fallback(&ctx.staged, convention, language),
            source: Source::Rules,
            attempts: Vec::new(),
        });
    }
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
        let checked = message::sanitize(&raw, convention).and_then(|message| {
            match copied_from(&message.subject, &ctx.subjects) {
                Some(subject) => Err(SanitizeError::CopiedHistory(subject.to_owned())),
                None => Ok(message),
            }
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
    Ok(Generation {
        message: message::fallback(&ctx.staged, convention, language),
        source: Source::Fallback,
        attempts,
    })
}

/// Whether the message for `avoid` comes without a model: from git, or
/// from the file list.
pub fn needs_model(ctx: &RepoContext, files: &[FileDiff], avoid: &[String]) -> bool {
    !avoid.is_empty() || (ctx.prepared.is_none() && !by_rules(ctx, files))
}

/// Git's prepared message as subject and body.
fn prepared_message(text: &str) -> CommitMessage {
    let (subject, body) = text.split_once('\n').unwrap_or((text, ""));
    let body = body.trim();
    CommitMessage {
        subject: subject.trim().to_owned(),
        body: (!body.is_empty()).then(|| body.to_owned()),
    }
}

/// Whether the message comes from the file list alone: every staged file
/// was written by a tool (see [`diff::machine_written`]).
pub fn by_rules(ctx: &RepoContext, files: &[FileDiff]) -> bool {
    !files.is_empty()
        && files
            .iter()
            .all(|f| diff::machine_written(f, &ctx.generated))
}

/// The history subject `subject` repeats, ignoring case and a final period.
fn copied_from<'a>(subject: &str, history: &'a [String]) -> Option<&'a str> {
    let normalize = |s: &str| s.trim().trim_end_matches('.').to_lowercase();
    let subject = normalize(subject);
    history
        .iter()
        .find(|h| normalize(h) == subject)
        .map(String::as_str)
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
        SanitizeError::CopiedHistory(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            format!(
                "\"{subject}\" repeats an earlier commit of this repository; \
                 describe what this diff changes instead"
            )
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
    fn a_copied_history_subject_is_retried() {
        // Real use: one commit of history, a one-line diff, and every model
        // repeated the history subject.
        let history = &["feat: Add a simple hello world program"];
        let mock = MockBackend::new([
            "feat: add a simple hello world program.",
            "feat: print the sum of 1 + 1",
        ]);
        let generation = run(history, &mock);
        assert_eq!(generation.source, Source::Model);
        assert_eq!(generation.message.subject, "feat: print the sum of 1 + 1");
        assert_eq!(
            generation.attempts[0].rejected,
            Some(SanitizeError::CopiedHistory(
                "feat: Add a simple hello world program".into()
            ))
        );
        assert!(mock.prompts()[1].user.ends_with(
            "Your previous reply was rejected because \"feat: Add a simple hello world \
             program\" repeats an earlier commit of this repository; describe what this \
             diff changes instead. Reply again with only the commit message in the \
             `type(scope): description` format.\n"
        ));
    }

    #[test]
    fn copying_twice_falls_back() {
        let mock = MockBackend::new(["fix(git): handle detached HEAD"; 2]);
        let generation = run(CONVENTIONAL, &mock);
        assert_eq!(generation.source, Source::Fallback);
        assert!(
            generation
                .attempts
                .iter()
                .all(|a| matches!(a.rejected, Some(SanitizeError::CopiedHistory(_))))
        );
    }

    #[test]
    fn a_subject_that_only_resembles_the_history_is_kept() {
        let mock = MockBackend::new(["fix(git): handle detached HEAD in hooks"]);
        let generation = run(CONVENTIONAL, &mock);
        assert_eq!(generation.attempts.len(), 1);
        assert_eq!(generation.source, Source::Model);
    }

    fn lockfile() -> Vec<FileDiff> {
        vec![FileDiff {
            path: "Cargo.lock".into(),
            old_path: None,
            lines: Some((7, 0)),
            hunks: "@@ -0,0 +1,7 @@\n+# This file is automatically @generated by Cargo.\n".into(),
        }]
    }

    fn lockfile_context() -> RepoContext {
        RepoContext::new(
            PathBuf::from("/repo"),
            None,
            vec!["feat: Add a simple hello world program".into()],
            vec![FileChange {
                status: ChangeStatus::Added,
                path: "Cargo.lock".into(),
                old_path: None,
            }],
        )
    }

    #[test]
    fn lockfiles_alone_do_not_ask_the_model() {
        // Real use: the model saw only `Cargo.lock | +7 -0` and repeated the
        // history twice; the file list was the right message all along.
        let mock = MockBackend::new(["feat: Add a simple hello world program"]);
        let generation = generate(
            &lockfile_context(),
            &lockfile(),
            &mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
        )
        .unwrap();
        assert_eq!(generation.source, Source::Rules);
        assert_eq!(generation.message.subject, "chore: add Cargo.lock");
        assert!(generation.attempts.is_empty());
        assert!(mock.prompts().is_empty());
    }

    #[test]
    fn git_prepared_messages_are_used_as_is() {
        use crate::git::{Operation, PreparedMessage};
        let ctx = context(CONVENTIONAL).with_prepared(Some(PreparedMessage {
            operation: Operation::Revert,
            message: "Revert \"fix(git): handle detached HEAD\"\n\nThis reverts commit 2e2ce44."
                .into(),
        }));
        let mock = MockBackend::new(["fix(git): read the branch name"]);
        assert!(!needs_model(&ctx, &files(), &[]));
        let generation = generate(
            &ctx,
            &files(),
            &mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
        )
        .unwrap();
        assert_eq!(generation.source, Source::Git);
        assert_eq!(
            generation.message.subject,
            "Revert \"fix(git): handle detached HEAD\""
        );
        assert_eq!(
            generation.message.body.as_deref(),
            Some("This reverts commit 2e2ce44.")
        );
        assert!(generation.attempts.is_empty() && mock.prompts().is_empty());
        // Turned down, it goes to the model for another.
        let avoid = [generation.message.subject.clone()];
        assert!(needs_model(&ctx, &files(), &avoid));
        let generation = generate_avoiding(
            &ctx,
            &files(),
            &mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &avoid,
        )
        .unwrap();
        assert_eq!(generation.source, Source::Model);
    }

    #[test]
    fn a_turned_down_lockfile_message_goes_to_the_model() {
        let mock = MockBackend::new(["chore(deps): pin the dependencies"]);
        let generation = generate_avoiding(
            &lockfile_context(),
            &lockfile(),
            &mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &["chore: add Cargo.lock".to_owned()],
        )
        .unwrap();
        assert_eq!(generation.source, Source::Model);
        assert_eq!(mock.prompts().len(), 1);
    }

    #[test]
    fn a_lockfile_with_code_still_asks_the_model() {
        let mut files = lockfile();
        files.extend(self::files());
        assert!(!by_rules(&lockfile_context(), &files));
        assert!(by_rules(&lockfile_context(), &lockfile()));
        assert!(!by_rules(&lockfile_context(), &[]));
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
