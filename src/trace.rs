//! Records a generation so it can be reproduced and checked later.
//!
//! A trace holds what git reported (branch, history, staged files and diff),
//! the options and generation config, and every attempt with the model's raw
//! reply. Everything between git and the model is deterministic, so
//! replaying the recorded replies must rebuild the same prompts and the same
//! message; any difference means the pipeline changed.
//!
//! Traces contain the staged diff and commit subjects. They are written only
//! on request, and the repository path is left out.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError, GenerationConfig, MockBackend};
use crate::context::RepoContext;
use crate::diff::FileDiff;
use crate::generate::{self, Generation};
use crate::git::{FileChange, PreparedMessage};
use crate::prompt::PromptOptions;

/// Bumped on incompatible changes to the trace layout.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub format: u32,
    /// Version of smart-commit that wrote the trace.
    pub tool_version: String,
    /// Model and device, from [`Backend::describe`].
    pub backend: String,
    pub options: PromptOptions,
    pub config: GenerationConfig,
    pub inputs: Inputs,
    pub generation: Generation,
}

/// What git reported, from which everything else is derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inputs {
    pub branch: Option<String>,
    pub subjects: Vec<String>,
    /// The paths each of `subjects` touched. Left out when empty, so older
    /// traces read, and replay, the same.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history_files: Vec<Vec<String>>,
    pub staged: Vec<FileChange>,
    pub files: Vec<FileDiff>,
    /// Staged paths `.gitattributes` marks `linguist-generated`. Left out
    /// when empty, so older traces read, and replay, the same.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<String>,
    /// The message git prepared for a merge, revert or cherry-pick in
    /// progress. Left out when there is none, so older traces read, and
    /// replay, the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared: Option<PreparedMessage>,
}

impl Inputs {
    /// The context these inputs describe, without a repository path.
    pub fn context(&self) -> RepoContext {
        RepoContext::new(
            PathBuf::new(),
            self.branch.clone(),
            self.subjects.clone(),
            self.staged.clone(),
        )
        .with_history_files(self.history_files.clone())
        .with_generated(self.generated.clone())
        .with_prepared(self.prepared.clone())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("invalid trace")]
    Json(#[from] serde_json::Error),
    #[error("trace format {found} is not supported (expected {FORMAT})")]
    UnsupportedFormat { found: u32 },
}

/// The outcome of replaying a trace.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub generation: Generation,
    /// Human-readable differences from the recorded generation; empty when
    /// it was reproduced exactly.
    pub differences: Vec<String>,
}

impl Trace {
    pub fn new(
        ctx: &RepoContext,
        files: &[FileDiff],
        backend: &dyn Backend,
        options: &PromptOptions,
        config: &GenerationConfig,
        generation: &Generation,
    ) -> Self {
        Self {
            format: FORMAT,
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            backend: backend.describe(),
            options: options.clone(),
            config: config.clone(),
            inputs: Inputs {
                branch: ctx.branch.clone(),
                subjects: ctx.subjects.clone(),
                history_files: ctx.history_files.clone(),
                staged: ctx.staged.clone(),
                files: files.to_vec(),
                generated: ctx.generated.clone(),
                prepared: ctx.prepared.clone(),
            },
            generation: generation.clone(),
        }
    }

    /// Pretty-printed, so traces diff well when kept as test fixtures.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("traces always serialize");
        json.push('\n');
        json
    }

    pub fn from_json(json: &str) -> Result<Self, TraceError> {
        #[derive(Deserialize)]
        struct Header {
            format: u32,
        }
        let header: Header = serde_json::from_str(json)?;
        if header.format != FORMAT {
            return Err(TraceError::UnsupportedFormat {
                found: header.format,
            });
        }
        Ok(serde_json::from_str(json)?)
    }

    /// The context the generation started from. The path is not recorded.
    pub fn context(&self) -> RepoContext {
        self.inputs.context()
    }

    /// Runs the generation again with the recorded replies in place of the
    /// model, and compares the result with the recorded one.
    pub fn replay(&self) -> Result<Replay, BackendError> {
        let mock = MockBackend::new(self.generation.attempts.iter().map(|a| a.raw.clone()));
        self.replay_with(&mock)
    }

    /// Like [`Self::replay`] with any backend, e.g. a real model to check
    /// that it still answers the same way.
    pub fn replay_with(&self, backend: &dyn Backend) -> Result<Replay, BackendError> {
        let generation = generate::generate_asked(
            &self.context(),
            &self.inputs.files,
            backend,
            &self.options,
            &self.config,
            &[],
            self.generation.asked.as_ref(),
        )?;
        let differences = compare(&self.generation, &generation);
        Ok(Replay {
            generation,
            differences,
        })
    }
}

fn compare(recorded: &Generation, replayed: &Generation) -> Vec<String> {
    let mut differences = Vec::new();
    let mut check = |what: String, same: bool| {
        if !same {
            differences.push(what);
        }
    };
    check(
        format!(
            "attempts: recorded {}, replayed {}",
            recorded.attempts.len(),
            replayed.attempts.len()
        ),
        recorded.attempts.len() == replayed.attempts.len(),
    );
    for (n, (old, new)) in recorded.attempts.iter().zip(&replayed.attempts).enumerate() {
        let n = n + 1;
        check(
            format!("attempt {n}: system prompt differs"),
            old.prompt.system == new.prompt.system,
        );
        check(
            format!("attempt {n}: user prompt differs"),
            old.prompt.user == new.prompt.user,
        );
        check(
            format!("attempt {n}: model reply differs"),
            old.raw == new.raw,
        );
        check(
            format!(
                "attempt {n}: recorded rejection {:?}, replayed {:?}",
                old.rejected, new.rejected
            ),
            old.rejected == new.rejected,
        );
    }
    check(
        format!(
            "source: recorded {:?}, replayed {:?}",
            recorded.source, replayed.source
        ),
        recorded.source == replayed.source,
    );
    check(
        format!(
            "message: recorded {:?}, replayed {:?}",
            recorded.message.to_string(),
            replayed.message.to_string()
        ),
        recorded.message == replayed.message,
    );
    differences
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn an_invalid_trace_keeps_the_parse_error_as_its_source() {
        let err = Trace::from_json("{").unwrap_err();
        assert_eq!(err.to_string(), "invalid trace");
        assert!(err.source().is_some());
    }
    use crate::git::ChangeStatus;

    fn recorded(replies: &[&str]) -> Trace {
        let ctx = RepoContext::new(
            PathBuf::from("/home/someone/repo"),
            Some("main".into()),
            vec!["feat(git): add history".into(), "fix(git): crash".into()],
            vec![FileChange {
                status: ChangeStatus::Modified,
                path: "src/git.rs".into(),
                old_path: None,
            }],
        );
        let files = vec![FileDiff {
            path: "src/git.rs".into(),
            old_path: None,
            lines: Some((1, 1)),
            hunks: "@@ -1 +1 @@\n-old\n+new\n".into(),
        }];
        let backend = MockBackend::new(replies.iter().copied());
        let options = PromptOptions::default();
        let config = GenerationConfig::default();
        let generation = generate::generate(&ctx, &files, &backend, &options, &config).unwrap();
        Trace::new(&ctx, &files, &backend, &options, &config, &generation)
    }

    #[test]
    fn json_round_trip() {
        let trace = recorded(&["Read the branch", "fix(git): read the branch"]);
        let json = trace.to_json();
        assert_eq!(Trace::from_json(&json).unwrap(), trace);
        assert!(!json.contains("/home/someone"), "the path is not recorded");
    }

    #[test]
    fn fresh_trace_replays_exactly() {
        for replies in [
            &["fix(git): read the branch"][..],
            &["Read the branch", "fix(git): read the branch"],
            &["", ""],
        ] {
            let replay = recorded(replies).replay().unwrap();
            assert_eq!(replay.differences, Vec::<String>::new(), "{replies:?}");
        }
    }

    #[test]
    fn changed_prompt_is_reported() {
        let mut trace = recorded(&["fix(git): read the branch"]);
        trace.generation.attempts[0].prompt.user.push_str("edited");
        assert_eq!(
            trace.replay().unwrap().differences,
            ["attempt 1: user prompt differs"]
        );
    }

    #[test]
    fn changed_outcome_is_reported() {
        let mut trace = recorded(&["fix(git): read the branch"]);
        trace.generation.message.subject = "fix(git): something else".into();
        let differences = trace.replay().unwrap().differences;
        assert_eq!(differences.len(), 1);
        assert!(differences[0].starts_with("message: recorded"));
    }

    #[test]
    fn changed_inputs_change_the_replay() {
        let mut trace = recorded(&["fix(git): read the branch"]);
        trace.inputs.subjects = vec!["Add history".into(), "Fix the crash".into()];
        let differences = trace.replay().unwrap().differences;
        assert!(differences.contains(&"attempt 1: system prompt differs".to_owned()));
    }

    #[test]
    fn history_files_are_recorded_and_optional() {
        let trace = recorded(&["fix(git): read the branch"]);
        assert!(!trace.to_json().contains("history_files"));

        let mut with_files = trace.clone();
        with_files.inputs.history_files = vec![vec!["src/git.rs".into()], vec![]];
        let json = with_files.to_json();
        assert!(json.contains("\"history_files\""));
        let read = Trace::from_json(&json).unwrap();
        assert_eq!(read, with_files);
        assert_eq!(
            read.context().history_files,
            with_files.inputs.history_files
        );
    }

    #[test]
    fn generated_paths_are_recorded_and_optional() {
        let trace = recorded(&["fix(git): read the branch"]);
        assert!(!trace.to_json().contains("\"generated\""));
        let mut marked = trace.clone();
        marked.inputs.generated = vec!["src/git.rs".into()];
        let read = Trace::from_json(&marked.to_json()).unwrap();
        assert_eq!(read, marked);
        assert_eq!(read.context().generated, ["src/git.rs"]);
    }

    #[test]
    fn unsupported_format_is_rejected() {
        let json = recorded(&[""])
            .to_json()
            .replacen("\"format\": 1", "\"format\": 99", 1);
        assert!(matches!(
            Trace::from_json(&json),
            Err(TraceError::UnsupportedFormat { found: 99 })
        ));
        assert!(matches!(Trace::from_json("{"), Err(TraceError::Json(_))));
    }

    #[test]
    fn a_chosen_type_is_recorded_and_replays() {
        let ctx = RepoContext::new(
            PathBuf::from("/home/someone/repo"),
            Some("main".into()),
            vec!["feat(git): add history".into(), "fix(git): crash".into()],
            vec![FileChange {
                status: ChangeStatus::Modified,
                path: "src/git.rs".into(),
                old_path: None,
            }],
        );
        let files = vec![FileDiff {
            path: "src/git.rs".into(),
            old_path: None,
            lines: Some((1, 1)),
            hunks: "@@ -1 +1 @@\n-old\n+new\n".into(),
        }];
        let asked = generate::TypeAsk {
            choices: vec![
                crate::backend::TypeChoice {
                    name: "feat".into(),
                    probability: 0.6,
                },
                crate::backend::TypeChoice {
                    name: "fix".into(),
                    probability: 0.4,
                },
            ],
            chosen: "fix".into(),
        };
        let backend = MockBackend::new(["fix(git): read the branch"]);
        let (options, config) = (PromptOptions::default(), GenerationConfig::default());
        let generation =
            generate::generate_asked(&ctx, &files, &backend, &options, &config, &[], Some(&asked))
                .unwrap();
        let trace = Trace::new(&ctx, &files, &backend, &options, &config, &generation);

        let json = trace.to_json();
        assert!(json.contains("\"chosen\": \"fix\""), "{json}");
        let read = Trace::from_json(&json).unwrap();
        assert_eq!(read, trace);
        let replay = read.replay().unwrap();
        assert!(replay.differences.is_empty(), "{:?}", replay.differences);
        assert_eq!(replay.generation.asked, Some(asked));
    }

    #[test]
    fn a_trace_without_a_chosen_type_has_no_such_field() {
        let trace = recorded(&["fix(git): read the branch"]);
        assert!(!trace.to_json().contains("asked"));
        assert_eq!(trace.generation.asked, None);
    }
}
