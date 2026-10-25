#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use std::path::PathBuf;

    use crate::backend::MockBackend;
    use crate::backend::{Backend, BackendError, GenerationConfig};
    use crate::context::RepoContext;
    use crate::diff::FileDiff;
    use crate::generate::{self, Generation, MAX_ATTEMPTS, Source, needs_model};
    use crate::git::{ChangeStatus, FileChange};
    use crate::message::SanitizeError;
    use crate::prompt::{Prompt, PromptOptions};
    use crate::style::{Convention, Language};

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
        crate::generate::generate(
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
        // The copied reply's type and scope stay; the file list says the rest.
        assert_eq!(generation.message.subject, "fix(git): update src/git.rs");
    }

    #[test]
    fn a_fallback_after_a_bad_format_keeps_the_file_lists_type() {
        let mock = MockBackend::new(["", "<think>hmm"]);
        let generation = run(CONVENTIONAL, &mock);
        assert_eq!(generation.source, Source::Fallback);
        assert_eq!(generation.message.subject, "chore: update src/git.rs");
    }

    #[test]
    fn a_near_copy_of_the_history_is_retried() {
        // Real use (commit scenarios): a directory rename answered with most
        // of an earlier subject, a few words dropped.
        let history = [
            "refatoração e estruturação de cameras, cenas, objetos e afins, falta modelar a cena da fase 1",
        ];
        let near = "refatoração e estruturação de objetos, falta modelar a cena da fase 1";
        assert_eq!(
            generate::copied_from(near, &history.map(String::from)),
            Some(history[0])
        );
        // Short replies and replies with words of their own are kept.
        let history = ["fix(git): handle detached HEAD".to_owned()];
        assert_eq!(
            generate::copied_from("fix(git): handle detached HEAD in hooks", &history),
            None
        );
        assert_eq!(
            generate::copied_from("fix(git): handle HEAD", &history),
            None
        );
    }

    #[test]
    fn a_reply_in_another_language_is_retried() {
        let history = [
            "adiciona pedidos",
            "corrige o total do pedido",
            "cria o cardápio",
        ];
        let mock = MockBackend::new(["add main", "torna o script principal executável"]);
        let generation = run(&history, &mock);
        assert_eq!(generation.source, Source::Model);
        assert_eq!(
            generation.message.subject,
            "torna o script principal executável"
        );
        assert_eq!(
            generation.attempts[0].rejected,
            Some(SanitizeError::WrongLanguage("add main".into()))
        );
        assert!(
            mock.prompts()[1]
                .user
                .ends_with("\"add main\" is not in Brazilian Portuguese; write it in Brazilian Portuguese. Reply again with only the commit message.\n"),
            "{}",
            mock.prompts()[1].user
        );
        // Mixed, as the history itself may be: kept.
        let mock = MockBackend::new(["fix do teclado"]);
        assert_eq!(run(&history, &mock).attempts.len(), 1);
        // A repository that also writes English accepts an English reply
        // (found on eval-v1).
        let mixed = [
            "adiciona pedidos",
            "corrige o total do pedido",
            "add the CLI",
        ];
        let mock = MockBackend::new(["add smtpSecure and smtpRequireTLS"]);
        assert_eq!(run(&mixed, &mock).attempts.len(), 1);
        // Without history the language is only the default (English): a
        // first commit written in Portuguese is kept.
        let mock = MockBackend::new(["feat: adiciona o sistema de pedidos"]);
        assert_eq!(run(&[], &mock).attempts.len(), 1);
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
        let generation = crate::generate::generate(
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
        let generation = crate::generate::generate(
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
        let generation = crate::generate::generate_avoiding(
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
    fn a_turned_down_message_is_not_written_again() {
        // Real use: asking for another message gave the same one back, the
        // note listing it notwithstanding.
        let avoid = ["fix(git): read the branch name from git\n\nUses --show-current.".to_owned()];
        let mock = MockBackend::new([
            "fix(git): read the branch name",
            "fix(git): use --show-current for the branch",
        ]);
        let generation = crate::generate::generate_avoiding(
            &context(CONVENTIONAL),
            &files(),
            &mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &avoid,
        )
        .unwrap();
        assert_eq!(generation.source, Source::Model);
        assert_eq!(
            generation.message.subject,
            "fix(git): use --show-current for the branch"
        );
        assert_eq!(
            generation.attempts[0].rejected,
            Some(SanitizeError::TurnedDown(
                "fix(git): read the branch name from git".into()
            ))
        );
        let retry = &mock.prompts()[1].user;
        assert!(
            retry.contains("The user turned down this message"),
            "{retry}"
        );
        assert!(
            retry.contains("\"fix(git): read the branch name from git\" is a message the user turned down; write a different one."),
            "{retry}"
        );

        // Repeated twice: the fallback, with the reply's type and scope, is
        // not the message turned down.
        let mock = MockBackend::new(["fix(git): read the branch name from git"; 2]);
        let generation = crate::generate::generate_avoiding(
            &context(CONVENTIONAL),
            &files(),
            &mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &avoid,
        )
        .unwrap();
        assert_eq!(generation.source, Source::Fallback);
        assert_eq!(generation.message.subject, "fix(git): update src/git.rs");
    }

    #[test]
    fn a_turned_down_lockfile_message_goes_to_the_model() {
        let mock = MockBackend::new(["chore(deps): pin the dependencies"]);
        let generation = crate::generate::generate_avoiding(
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
        assert!(!generate::by_rules(&lockfile_context(), &files));
        assert!(generate::by_rules(&lockfile_context(), &lockfile()));
        assert!(!generate::by_rules(&lockfile_context(), &[]));
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

    /// A backend that takes four fifths of the budget it is asked for, as the
    /// NPU does, and records the prompts it was given.
    struct Tight(MockBackend);

    impl Backend for Tight {
        fn describe(&self) -> String {
            "tight".into()
        }

        fn generate(
            &self,
            prompt: &Prompt,
            config: &GenerationConfig,
        ) -> Result<String, BackendError> {
            self.0.generate(prompt, config)
        }

        fn prompt_budget(&self, requested: usize) -> usize {
            requested * 4 / 5
        }
    }

    #[test]
    fn a_backend_can_take_less_than_the_budget_asked_for() {
        let options = PromptOptions::default();
        let tight = Tight(MockBackend::new(["fix(git): x"]));
        let budgeted = crate::generate::budgeted(&options, &tight);
        assert_eq!(
            budgeted.max_prompt_tokens,
            options.max_prompt_tokens * 4 / 5
        );
        // Nothing else changes, and a backend that takes all keeps the options.
        assert_eq!(
            PromptOptions {
                max_prompt_tokens: options.max_prompt_tokens,
                ..budgeted
            },
            options
        );
        assert_eq!(
            crate::generate::budgeted(&options, &MockBackend::new(["x"])),
            options
        );
    }

    #[test]
    fn a_tighter_budget_cuts_the_diff_sooner() {
        let big = vec![FileDiff {
            path: "src/git.rs".into(),
            old_path: None,
            lines: Some((400, 0)),
            hunks: format!(
                "@@ -0,0 +1,400 @@\n{}",
                "+let value = compute(1);\n".repeat(400)
            ),
        }];
        let tokens = |budget: usize| {
            let mock = MockBackend::new(["fix(git): x"]);
            let options = PromptOptions {
                max_prompt_tokens: budget,
                ..PromptOptions::default()
            };
            crate::generate::generate(
                &context(CONVENTIONAL),
                &big,
                &mock,
                &options,
                &GenerationConfig::default(),
            )
            .unwrap();
            let prompt = &mock.prompts()[0];
            crate::diff::estimate_tokens(&prompt.system)
                + crate::diff::estimate_tokens(&prompt.user)
        };
        let (full, tight) = (tokens(1024), tokens(819));
        assert!(full <= 1024 && tight <= 819, "{full} {tight}");
        assert!(tight < full);
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
        let result = crate::generate::generate(
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
        assert_eq!(generate::avoid_note(&[]), None);
        assert_eq!(
            generate::avoid_note(&["feat: add a\n\nbody".to_owned()]).unwrap(),
            "The user turned down this message: \"feat: add a\". Write a different \
             message that describes the change better."
        );
        let two = generate::avoid_note(&["feat: a".to_owned(), "fix: b".to_owned()]).unwrap();
        assert!(two.starts_with("The user turned down these messages: \"feat: a\"; \"fix: b\"."));

        let ctx = context(&["feat: add login"]);
        let files = files();
        let backend = MockBackend::new(["feat: add retry"]);
        let avoid = vec!["feat: add loop".to_owned()];
        let generation = crate::generate::generate_avoiding(
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
        assert!(
            user.contains(&generate::avoid_note(&avoid).unwrap()),
            "{user}"
        );
        let again = crate::generate::generate_avoiding(
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
        let generation = crate::generate::generate_avoiding(
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
            generate::retry_note(&SanitizeError::Empty, Convention::Free, Language::English),
            "Your previous reply was rejected because it contained no commit message. \
             Reply again with only the commit message."
        );
    }

    fn asked(chosen: &str) -> generate::TypeAsk {
        generate::TypeAsk {
            choices: vec![
                crate::backend::TypeChoice {
                    name: "feat".into(),
                    probability: 0.55,
                },
                crate::backend::TypeChoice {
                    name: "fix".into(),
                    probability: 0.45,
                },
            ],
            chosen: chosen.to_owned(),
        }
    }

    fn run_asked(mock: &MockBackend, asked: Option<&generate::TypeAsk>) -> Generation {
        generate::generate_asked(
            &context(CONVENTIONAL),
            &files(),
            mock,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &[],
            asked,
        )
        .unwrap()
    }

    #[test]
    fn a_chosen_type_begins_the_reply_and_is_recorded() {
        let mock = MockBackend::new(["fix(git): read the branch name"]);
        let generation = run_asked(&mock, Some(&asked("fix")));
        assert_eq!(generation.message.subject, "fix(git): read the branch name");
        assert_eq!(mock.starts(), ["fix"]);
        assert_eq!(generation.asked, Some(asked("fix")));
    }

    #[test]
    fn the_retry_begins_with_the_chosen_type_too() {
        let mock = MockBackend::new(["Read the branch name", "fix(git): read the branch name"]);
        let generation = run_asked(&mock, Some(&asked("fix")));
        assert_eq!(generation.attempts.len(), 2);
        assert_eq!(mock.starts(), ["fix", "fix"]);
        assert_eq!(generation.asked, Some(asked("fix")));
    }

    #[test]
    fn the_fallback_records_the_chosen_type_too() {
        let mock = MockBackend::new(["", ""]);
        let generation = run_asked(&mock, Some(&asked("docs")));
        assert_eq!(generation.source, Source::Fallback);
        assert_eq!(generation.asked, Some(asked("docs")));
    }

    #[test]
    fn without_a_chosen_type_nothing_is_started_or_recorded() {
        let mock = MockBackend::new(["fix(git): read the branch name"]);
        let generation = run_asked(&mock, None);
        assert!(mock.starts().is_empty());
        assert_eq!(generation.asked, None);
        let json = serde_json::to_string(&generation).unwrap();
        assert!(!json.contains("asked"), "{json}");
    }

    #[test]
    fn a_backend_that_cannot_start_a_reply_is_an_error_not_a_silent_other_type() {
        let err = generate::generate_asked(
            &context(CONVENTIONAL),
            &files(),
            &crate::backend::NoModel,
            &PromptOptions::default(),
            &GenerationConfig::default(),
            &[],
            Some(&asked("fix")),
        )
        .unwrap_err();
        assert!(matches!(err, BackendError::Unavailable(_)), "{err}");
    }
}
