//! Asking which type a message should have, when the model is unsure.
//!
//! The first token of a reply is where the model picks the type, and how
//! sure it is shows: on eval-v1 it was right 94% of the time above 90%
//! confidence and 49% below 50%, and the real type was among its two likeliest
//! 90% of the time. So below some confidence the person is offered those
//! types, and the reply is made to begin with the one chosen.

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, TypeChoice};
use crate::context::RepoContext;
use crate::diff::FileDiff;
use crate::prompt::{self, PromptOptions};
use crate::style::Convention;

/// Offered at most this many types: the real one was among the two likeliest
/// 90% of the time and the three likeliest 95%.
pub const MAX_OFFERED: usize = 4;

/// A type less likely than this is not worth a line.
const MIN_OFFERED: f64 = 0.02;

/// The type a person chose among those offered, which the reply is made to
/// begin with, and what was offered. Recorded in traces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeAsk {
    pub choices: Vec<TypeChoice>,
    pub chosen: String,
}

/// The types to offer a person, most likely first, when the model is less
/// sure than `below` (0 to 1) of the likeliest; `None` when it is sure
/// enough, or has nothing to choose between.
///
/// `allowed` are the types the repository uses (all of them when empty):
/// others are left out and the rest count again to 1.
pub fn types_to_offer(
    choices: &[TypeChoice],
    allowed: &[String],
    below: f64,
) -> Option<Vec<TypeChoice>> {
    let mut kept: Vec<TypeChoice> = choices
        .iter()
        .filter(|c| allowed.is_empty() || allowed.contains(&c.name))
        .cloned()
        .collect();
    let total: f64 = kept.iter().map(|c| c.probability).sum();
    if total <= 0.0 {
        return None;
    }
    for choice in &mut kept {
        choice.probability /= total;
    }
    if kept.first()?.probability >= below {
        return None;
    }
    kept.retain(|c| c.probability >= MIN_OFFERED);
    kept.truncate(MAX_OFFERED);
    (kept.len() >= 2).then_some(kept)
}

/// The types to offer a person before the message for `files` is written,
/// if `backend` can tell how likely each is, the repository writes
/// Conventional Commits and it is less sure than `below` of the likeliest.
/// Never an error: a backend that fails to tell is as one that cannot, and
/// the message is written as always.
pub fn offer_types(
    ctx: &RepoContext,
    files: &[FileDiff],
    backend: &dyn Backend,
    options: &PromptOptions,
    below: f64,
) -> Option<Vec<TypeChoice>> {
    let convention = prompt::effective_convention(&ctx.style, options);
    if convention != Convention::Conventional {
        return None;
    }
    let choices = match backend.type_choices(&prompt::build(ctx, files, options)) {
        Ok(choices) => choices,
        Err(err) => {
            tracing::warn!("cannot tell how sure the model is of the type: {err}");
            return None;
        }
    };
    types_to_offer(&choices, &prompt::listed_types(&ctx.style), below)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(list: &[(&str, f64)]) -> Vec<TypeChoice> {
        list.iter()
            .map(|(name, probability)| TypeChoice {
                name: (*name).to_owned(),
                probability: *probability,
            })
            .collect()
    }

    fn names(offered: &[TypeChoice]) -> Vec<&str> {
        offered.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn a_model_that_is_sure_is_not_asked() {
        let sure = choices(&[("fix", 0.92), ("feat", 0.06), ("docs", 0.02)]);
        assert_eq!(types_to_offer(&sure, &[], 0.5), None);
    }

    #[test]
    fn a_model_that_is_unsure_offers_its_likeliest_types() {
        let unsure = choices(&[
            ("feat", 0.45),
            ("fix", 0.35),
            ("refactor", 0.15),
            ("docs", 0.05),
        ]);
        let offered = types_to_offer(&unsure, &[], 0.5).unwrap();
        assert_eq!(names(&offered), ["feat", "fix", "refactor", "docs"]);
    }

    #[test]
    fn the_confidence_that_asks_is_the_one_given() {
        let middling = choices(&[("feat", 0.6), ("fix", 0.4)]);
        assert_eq!(types_to_offer(&middling, &[], 0.5), None);
        assert!(types_to_offer(&middling, &[], 0.7).is_some());
        // Exactly at the limit is sure enough.
        assert_eq!(
            types_to_offer(&choices(&[("feat", 0.5), ("fix", 0.5)]), &[], 0.5),
            None
        );
    }

    #[test]
    fn offers_are_limited_and_leave_out_the_unlikely() {
        let many = choices(&[
            ("feat", 0.30),
            ("fix", 0.25),
            ("docs", 0.2),
            ("chore", 0.15),
            ("refactor", 0.07),
            ("test", 0.01),
            ("perf", 0.01),
            ("ci", 0.01),
        ]);
        let offered = types_to_offer(&many, &[], 0.5).unwrap();
        assert_eq!(names(&offered), ["feat", "fix", "docs", "chore"]);

        let few = choices(&[
            ("feat", 0.45),
            ("fix", 0.45),
            ("docs", 0.06),
            ("ci", 0.01),
            ("test", 0.01),
            ("perf", 0.01),
            ("style", 0.01),
        ]);
        let offered = types_to_offer(&few, &[], 0.5).unwrap();
        assert_eq!(names(&offered), ["feat", "fix", "docs"]);
    }

    #[test]
    fn only_the_repositorys_types_are_offered_and_count_again_to_one() {
        let model = choices(&[("feat", 0.5), ("revert", 0.3), ("fix", 0.2)]);
        let allowed = vec!["feat".to_owned(), "fix".to_owned(), "docs".to_owned()];
        let offered = types_to_offer(&model, &allowed, 0.8).unwrap();
        assert_eq!(names(&offered), ["feat", "fix"]);
        let total: f64 = offered.iter().map(|c| c.probability).sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!((offered[0].probability - 0.5 / 0.7).abs() < 1e-9);
        // The model was less sure than it looks only because of a type the
        // repository does not use: with it gone, 0.71 passes 0.7.
        assert_eq!(types_to_offer(&model, &allowed, 0.7), None);
    }

    #[test]
    fn nothing_to_choose_between_is_not_asked() {
        assert_eq!(types_to_offer(&[], &[], 0.5), None);
        assert_eq!(types_to_offer(&choices(&[("fix", 0.4)]), &[], 0.5), None);
        let model = choices(&[("feat", 0.5), ("fix", 0.5)]);
        let allowed = vec!["docs".to_owned()];
        assert_eq!(types_to_offer(&model, &allowed, 0.9), None);
        // One left after the allowed ones: nothing to choose.
        let allowed = vec!["feat".to_owned()];
        assert_eq!(types_to_offer(&model, &allowed, 1.1), None);
    }

    mod offer {
        use super::*;
        use crate::backend::{MockBackend, NoModel};
        use crate::diff::FileDiff;
        use crate::git::{ChangeStatus, FileChange};
        use crate::prompt::PromptOptions;
        use std::path::PathBuf;

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

        const CONVENTIONAL: &[&str] = &["feat(git): add history", "fix(git): handle HEAD"];

        fn offer(subjects: &[&str], backend: &dyn Backend, below: f64) -> Option<Vec<TypeChoice>> {
            offer_types(
                &context(subjects),
                &files(),
                backend,
                &PromptOptions::default(),
                below,
            )
        }

        #[test]
        fn an_unsure_model_on_a_conventional_repository_is_offered_its_types() {
            let backend = MockBackend::default().with_type_choices([
                ("feat", 0.5),
                ("fix", 0.4),
                ("docs", 0.1),
            ]);
            let offered = offer(CONVENTIONAL, &backend, 0.6).unwrap();
            // docs is not a type this repository uses; refactor never was offered.
            assert_eq!(names(&offered), ["feat", "fix"]);
        }

        #[test]
        fn a_sure_model_is_not_asked() {
            let backend = MockBackend::default().with_type_choices([("fix", 0.9), ("feat", 0.1)]);
            assert_eq!(offer(CONVENTIONAL, &backend, 0.5), None);
        }

        #[test]
        fn a_backend_that_cannot_tell_is_not_asked() {
            assert_eq!(offer(CONVENTIONAL, &NoModel, 0.99), None);
            assert_eq!(offer(CONVENTIONAL, &MockBackend::default(), 0.99), None);
        }

        #[test]
        fn only_conventional_commits_have_types_to_choose() {
            let backend = MockBackend::default().with_type_choices([("feat", 0.5), ("fix", 0.5)]);
            let free = ["Add history", "Handle detached HEAD", "Read the branch"];
            assert_eq!(offer(&free, &backend, 0.99), None);
            let gitmoji = ["✨ add history", "🐛 handle HEAD"];
            assert_eq!(offer(&gitmoji, &backend, 0.99), None);
        }

        #[test]
        fn a_repository_without_history_offers_every_type() {
            let backend = MockBackend::default().with_type_choices([
                ("feat", 0.4),
                ("fix", 0.35),
                ("chore", 0.25),
            ]);
            let options = PromptOptions {
                convention: Some(Convention::Conventional),
                ..PromptOptions::default()
            };
            let offered = offer_types(&context(&[]), &files(), &backend, &options, 0.5).unwrap();
            assert_eq!(names(&offered), ["feat", "fix", "chore"]);
        }

        #[test]
        fn a_backend_that_fails_is_not_asked_and_does_not_fail() {
            struct Failing;
            impl Backend for Failing {
                fn describe(&self) -> String {
                    "failing".into()
                }
                fn generate(
                    &self,
                    _: &crate::prompt::Prompt,
                    _: &crate::backend::GenerationConfig,
                ) -> Result<String, crate::backend::BackendError> {
                    Ok(String::new())
                }
                fn type_choices(
                    &self,
                    _: &crate::prompt::Prompt,
                ) -> Result<Vec<TypeChoice>, crate::backend::BackendError> {
                    Err(crate::backend::BackendError::Generation("boom".into()))
                }
            }
            assert_eq!(offer(CONVENTIONAL, &Failing, 0.99), None);
        }
    }
}
