//! Turns the repository context and the staged diff into a chat prompt.
//!
//! Instructions are written in English, which small models follow best,
//! and ask for the message in the repository's own language.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::context::RepoContext;
use crate::diff::{self, CompactDiff, FileDiff};
use crate::style::{self, Convention, Language, Style, TicketStyle};

/// The NPU pipeline's default `MAX_PROMPT_LEN`.
pub const DEFAULT_MAX_PROMPT_TOKENS: usize = 1024;
/// Room for the chat template's role markers around our two messages.
const TEMPLATE_OVERHEAD_TOKENS: usize = 40;
/// Types and scopes beyond these are rarely used and only cost tokens.
const MAX_LISTED: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// How much of the fixed instructions the system message carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptRules {
    /// Every rule, for models that were not trained on smart-commit's task.
    #[default]
    Full,
    /// Only what varies by repository (types, scopes, examples) and what
    /// training does not teach (a ticket, Portuguese, gitmoji), for a model
    /// trained to follow the rest without being told: a shorter prompt is
    /// read faster. Never for an untrained model, which would lose the
    /// format (training H6).
    Omitted,
}

impl PromptRules {
    pub fn is_full(&self) -> bool {
        *self == Self::Full
    }
}

/// How the file list describes each file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffFormat {
    /// `path | +3 -1`, what the fine-tunes were trained on.
    #[default]
    Plain,
    /// With each file's status, renames as `dir/{old => new}`, and
    /// whitespace-only and comment-only changes named (see `annotate`).
    Annotated,
}

impl DiffFormat {
    pub fn is_plain(&self) -> bool {
        *self == Self::Plain
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptOptions {
    pub max_prompt_tokens: usize,
    /// Used when the history is too short or mixed to tell.
    pub default_convention: Convention,
    pub default_language: Language,
    /// Forces a convention instead of the detected one. Left out of traces
    /// when unset, so older traces still read the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convention: Option<Convention>,
    /// Forces a language instead of the detected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// Left out of traces when full, so older traces still read the same.
    #[serde(default, skip_serializing_if = "PromptRules::is_full")]
    pub rules: PromptRules,
    /// Left out of traces when plain, so older traces still read the same.
    #[serde(default, skip_serializing_if = "DiffFormat::is_plain")]
    pub diff: DiffFormat,
}

impl Default for PromptOptions {
    fn default() -> Self {
        Self {
            max_prompt_tokens: DEFAULT_MAX_PROMPT_TOKENS,
            default_convention: Convention::Conventional,
            default_language: Language::English,
            convention: None,
            language: None,
            rules: PromptRules::Full,
            diff: DiffFormat::Plain,
        }
    }
}

/// The convention and language the message must follow: forced ones
/// first, then the detected ones, then the defaults.
pub fn effective_style(style: &Style, options: &PromptOptions) -> (Convention, Language) {
    let convention = options.convention.unwrap_or(match style.convention {
        Convention::Unknown => options.default_convention,
        known => known,
    });
    let language = options.language.unwrap_or(match style.language {
        Language::Unknown => options.default_language,
        known => known,
    });
    (convention, language)
}

/// Examples that follow `convention`: when it is forced, history written
/// in another style would contradict the rules.
fn examples_for(style: &Style, convention: Convention) -> impl Iterator<Item = &String> {
    style
        .examples
        .iter()
        .filter(move |example| match convention {
            Convention::Conventional => style::parse_conventional(example).is_some(),
            Convention::Gitmoji => style::is_gitmoji(example),
            Convention::Free | Convention::Unknown => true,
        })
}

pub fn build(ctx: &RepoContext, files: &[FileDiff], options: &PromptOptions) -> Prompt {
    build_with_note(ctx, files, options, None)
}

/// Like [`build`], with `note` at the end of the user message, e.g. why the
/// previous reply was rejected. The diff shrinks to make room for it.
pub fn build_with_note(
    ctx: &RepoContext,
    files: &[FileDiff],
    options: &PromptOptions,
    note: Option<&str>,
) -> Prompt {
    let system = system_message(ctx, options);
    // Sized with the truncation note at its longest, which large diffs
    // will need.
    let empty = CompactDiff {
        truncated: usize::MAX,
        ..CompactDiff::default()
    };
    let fixed = diff::estimate_tokens(&system)
        + diff::estimate_tokens(&user_message(&empty, note))
        + TEMPLATE_OVERHEAD_TOKENS;
    let budget = options.max_prompt_tokens.saturating_sub(fixed);
    let compacted = match options.diff {
        DiffFormat::Plain => diff::compact(files, budget),
        DiffFormat::Annotated => diff::compact_annotated(files, &ctx.staged, budget),
    };
    let user = user_message(&compacted, note);
    Prompt { system, user }
}

fn system_message(ctx: &RepoContext, options: &PromptOptions) -> String {
    let style = &ctx.style;
    let (convention, language) = effective_style(style, options);
    let full = options.rules.is_full();
    let mut text = if full {
        String::from(
            "You write git commit messages from a diff. Reply with the commit \
             message only: no code fences, no quotes, no explanation.\n\nRules:\n",
        )
    } else {
        String::new()
    };
    let mut rule = |line: &str| {
        let _ = writeln!(text, "- {line}");
    };

    if full {
        rule("Subject line of at most 72 characters, without a trailing period.");
    }
    match language {
        Language::Portuguese => rule("Write in Brazilian Portuguese."),
        _ if full => rule("Write in English, in the imperative mood (\"Add\", not \"Added\")."),
        _ => {}
    }
    match convention {
        Convention::Conventional => {
            rule(if full {
                "Use Conventional Commits: `type(scope): description`."
            } else {
                "Use Conventional Commits."
            });
            if !style.types.is_empty() {
                rule(&format!("Types used here: {}.", names(&style.types)));
            } else if full {
                rule("Types: feat, fix, docs, refactor, test, chore.");
            }
            if !style.scopes.is_empty() {
                rule(&if full {
                    format!(
                        "Scopes used here: {}. Omit the scope if none fits.",
                        names(&style.scopes)
                    )
                } else {
                    format!("Scopes used here: {}.", names(&style.scopes))
                });
            }
        }
        Convention::Gitmoji => rule(
            "Start the subject with one gitmoji for the change: \
             ✨ feature, 🐛 fix, ♻️ refactor, 📝 docs, ✅ tests, 🔧 config.",
        ),
        Convention::Free | Convention::Unknown => {}
    }
    if let Some(ticket) = &ctx.ticket {
        match (style.tickets, convention) {
            (TicketStyle::Prefix, Convention::Conventional) => rule(&format!(
                "Start the description with {ticket}, right after `type(scope): `."
            )),
            (TicketStyle::Prefix, Convention::Gitmoji) => rule(&format!(
                "Start the description with {ticket}, right after the gitmoji."
            )),
            (TicketStyle::Prefix, _) => rule(&format!("Start the subject with {ticket}.")),
            (TicketStyle::Mention, _) => rule(&format!("Mention {ticket} in the subject.")),
            (TicketStyle::None, _) => {}
        }
    }
    if full {
        rule("Add a body only if the reason for the change is not obvious.");
    }

    let mut examples = examples_for(style, convention).peekable();
    if examples.peek().is_some() {
        text.push_str(if full {
            "\nRecent subjects from this repository, follow their style:\n"
        } else {
            "\nRecent subjects from this repository:\n"
        });
        for example in examples {
            let _ = writeln!(text, "{example}");
        }
    }
    text
}

fn user_message(diff: &CompactDiff, note: Option<&str>) -> String {
    let mut text = format!("Files changed:\n{}", diff.stat);
    if !diff.patch.is_empty() {
        let _ = write!(text, "\nDiff:\n{}", diff.patch);
    }
    if diff.truncated > 0 {
        let _ = write!(
            text,
            "\n({} files are only partly shown; rely on the file list.)\n",
            diff.truncated
        );
    }
    if let Some(note) = note {
        let _ = write!(text, "\n{note}\n");
    }
    text
}

fn names(ranked: &[(String, usize)]) -> String {
    ranked
        .iter()
        .take(MAX_LISTED)
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::style;

    fn context(subjects: &[&str], branch: Option<&str>) -> RepoContext {
        RepoContext::new(
            PathBuf::from("/repo"),
            branch.map(str::to_owned),
            subjects.iter().map(|s| s.to_string()).collect(),
            Vec::new(),
        )
    }

    fn files() -> Vec<FileDiff> {
        vec![
            FileDiff {
                path: "src/git.rs".into(),
                old_path: None,
                lines: Some((2, 1)),
                hunks: "@@ -80,3 +80,4 @@\n-        let out = self.git(&[\"branch\"])?;\n\
                        +        let out = self.git(&[\"branch\", \"--show-current\"])?;\n\
                        +        let name = out.trim();\n"
                    .into(),
            },
            FileDiff {
                path: "Cargo.lock".into(),
                old_path: None,
                lines: Some((40, 3)),
                hunks: "@@ -1 +1 @@\n-x\n+y\n".into(),
            },
        ]
    }

    #[test]
    fn annotated_file_list() {
        use crate::git::{ChangeStatus, FileChange};
        let staged = vec![
            FileChange {
                status: ChangeStatus::Modified,
                path: "src/git.rs".into(),
                old_path: None,
            },
            FileChange {
                status: ChangeStatus::Renamed,
                path: "src/vcs/repo.rs".into(),
                old_path: Some("src/repo.rs".into()),
            },
            FileChange {
                status: ChangeStatus::Modified,
                path: "src/style.rs".into(),
                old_path: None,
            },
        ];
        let ctx = RepoContext::new(
            PathBuf::from("/repo"),
            Some("main".into()),
            vec![
                "feat(git): read the branch".into(),
                "fix(diff): keep renames".into(),
            ],
            staged,
        );
        let mut changed = files();
        changed.truncate(1);
        changed.push(FileDiff {
            path: "src/vcs/repo.rs".into(),
            old_path: Some("src/repo.rs".into()),
            lines: Some((0, 0)),
            hunks: String::new(),
        });
        changed.push(FileDiff {
            path: "src/style.rs".into(),
            old_path: None,
            lines: Some((1, 1)),
            hunks: "@@ -3 +3 @@\n-fn detect(s: &[String]) -> Style {\n+fn detect(s: &[String]) -> Style {   \n"
                .into(),
        });
        let options = PromptOptions {
            diff: DiffFormat::Annotated,
            ..PromptOptions::default()
        };
        let prompt = build(&ctx, &changed, &options);
        insta::assert_snapshot!(prompt.user);
        // The plain list of the same change keeps the format trained on.
        let plain = build(&ctx, &changed, &PromptOptions::default());
        assert!(
            plain
                .user
                .contains("src/repo.rs => src/vcs/repo.rs | +0 -0\n")
        );
        assert!(plain.user.contains("--- src/style.rs"));
    }

    #[test]
    fn conventional_portuguese_repository() {
        let ctx = context(
            &[
                "feat(cli): adiciona o comando context",
                "fix(git): corrige a leitura da branch",
                "docs: atualiza a documentação da fase 1",
            ],
            Some("feature/login"),
        );
        let prompt = build(&ctx, &files(), &PromptOptions::default());
        insta::assert_snapshot!(format!("{}\n=====\n{}", prompt.system, prompt.user));
    }

    #[test]
    fn scopes_are_listed_without_their_places() {
        // The places of each scope are recorded but not shown: alone they
        // did not help the models (training H4), and cost prompt tokens.
        let subjects = ["feat(cli): add context", "fix(git): read branch"];
        let paths = vec![
            vec!["src/main.rs".to_string()],
            vec!["src/git.rs".to_string()],
        ];
        let ctx = context(&subjects, None).with_history_files(paths);
        let prompt = build(&ctx, &files(), &PromptOptions::default());
        assert!(
            prompt
                .system
                .contains("- Scopes used here: cli, git. Omit the scope if none fits.\n"),
            "{}",
            prompt.system
        );
    }

    fn omitted() -> PromptOptions {
        PromptOptions {
            rules: PromptRules::Omitted,
            ..PromptOptions::default()
        }
    }

    #[test]
    fn omitted_rules_keep_only_what_varies_by_repository() {
        let ctx = context(
            &[
                "feat(cli): add context",
                "fix(git): read branch",
                "docs: describe the hook",
            ],
            None,
        );
        let prompt = build(&ctx, &files(), &omitted());
        insta::assert_snapshot!(format!("{}\n=====\n{}", prompt.system, prompt.user));
        let full = build(&ctx, &files(), &PromptOptions::default());
        assert!(prompt.system.len() < full.system.len());
        // The diff is the same: only the instructions differ.
        assert!(prompt.user.contains("Diff:\n--- src/git.rs"));
    }

    #[test]
    fn omitted_rules_keep_what_training_does_not_teach() {
        // Tickets, Portuguese and gitmoji are absent from the training data,
        // so the model still needs to be told.
        let gitmoji = context(
            &["✨ ABC-1 add login page", "🐛 ABC-2 fix crash on logout"],
            Some("feature/ABC-3-signup"),
        );
        let prompt = build(&gitmoji, &files(), &omitted());
        assert!(prompt.system.contains("Start the subject with one gitmoji"));
        assert!(
            prompt
                .system
                .contains("Start the description with ABC-3, right after the gitmoji.")
        );
        let portuguese = context(&["feat(cli): adiciona o comando context"], None);
        let options = PromptOptions {
            language: Some(Language::Portuguese),
            ..omitted()
        };
        let prompt = build(&portuguese, &files(), &options);
        assert!(prompt.system.contains("- Write in Brazilian Portuguese.\n"));
        // An English, Conventional repository without history needs no list.
        let prompt = build(&context(&[], None), &files(), &omitted());
        assert_eq!(prompt.system, "- Use Conventional Commits.\n");
    }

    #[test]
    fn full_rules_are_left_out_of_recorded_options() {
        let full = serde_json::to_value(PromptOptions::default()).unwrap();
        assert!(full.get("rules").is_none(), "{full}");
        let value = serde_json::to_value(omitted()).unwrap();
        assert_eq!(value["rules"], "omitted");
        let back: PromptOptions = serde_json::from_value(value).unwrap();
        assert_eq!(back.rules, PromptRules::Omitted);
        // Options recorded before rules existed read as full.
        let old: PromptOptions = serde_json::from_value(full).unwrap();
        assert_eq!(old, PromptOptions::default());
    }

    #[test]
    fn empty_history_uses_defaults() {
        let ctx = context(&[], None);
        let prompt = build(&ctx, &files(), &PromptOptions::default());
        insta::assert_snapshot!(prompt.system);
    }

    #[test]
    fn gitmoji_repository_with_ticket_prefix() {
        let ctx = context(
            &["✨ ABC-1 add login page", "🐛 ABC-2 fix crash on logout"],
            Some("feature/ABC-3-signup"),
        );
        let prompt = build(&ctx, &files(), &PromptOptions::default());
        assert!(prompt.system.contains("Start the subject with one gitmoji"));
        assert!(
            prompt
                .system
                .contains("Start the description with ABC-3, right after the gitmoji.")
        );
    }

    #[test]
    fn ticket_prefix_and_mention_in_free_style() {
        let prefixed = context(&["ABC-1: add login", "[ABC-2] fix crash"], Some("ABC-3"));
        let prompt = build(&prefixed, &files(), &PromptOptions::default());
        assert!(prompt.system.contains("Start the subject with ABC-3."));

        let mentioned = context(&["add login (ABC-1)", "fix crash, ABC-2"], Some("ABC-3"));
        let prompt = build(&mentioned, &files(), &PromptOptions::default());
        assert!(prompt.system.contains("Mention ABC-3 in the subject."));
    }

    #[test]
    fn forced_style_overrides_the_detected_one() {
        let options = PromptOptions {
            convention: Some(Convention::Conventional),
            language: Some(Language::Portuguese),
            ..PromptOptions::default()
        };
        let english_free = style::detect(&["Add the parser", "Fix the crash"]);
        assert_eq!(
            effective_style(&english_free, &options),
            (Convention::Conventional, Language::Portuguese)
        );
    }

    #[test]
    fn forced_convention_drops_examples_that_contradict_it() {
        let ctx = context(
            &["feat: add the parser", "Fix the crash on empty input"],
            None,
        );
        let forced = PromptOptions {
            convention: Some(Convention::Conventional),
            ..PromptOptions::default()
        };
        let prompt = build(&ctx, &files(), &forced);
        assert!(prompt.system.contains("Use Conventional Commits"));
        assert!(prompt.system.contains("feat: add the parser"));
        assert!(!prompt.system.contains("Fix the crash on empty input"));

        // Gitmoji forced on a history without any: no examples at all.
        let gitmoji = PromptOptions {
            convention: Some(Convention::Gitmoji),
            ..PromptOptions::default()
        };
        let prompt = build(&ctx, &files(), &gitmoji);
        assert!(!prompt.system.contains("Recent subjects"));
    }

    #[test]
    fn unset_overrides_are_left_out_of_traces() {
        let json = serde_json::to_string(&PromptOptions::default()).unwrap();
        assert!(!json.contains("\"convention\""), "{json}");
        let old: PromptOptions = serde_json::from_str(
            r#"{"max_prompt_tokens":1024,"default_convention":"conventional","default_language":"english"}"#,
        )
        .unwrap();
        assert_eq!(old, PromptOptions::default());
    }

    #[test]
    fn known_style_overrides_the_defaults() {
        let options = PromptOptions {
            default_convention: Convention::Gitmoji,
            default_language: Language::Portuguese,
            ..PromptOptions::default()
        };
        let empty = style::detect::<&str>(&[]);
        assert_eq!(
            effective_style(&empty, &options),
            (Convention::Gitmoji, Language::Portuguese)
        );
        let english = style::detect(&["Add the parser", "Fix the crash"]);
        assert_eq!(
            effective_style(&english, &options),
            (Convention::Free, Language::English)
        );
    }

    #[test]
    fn ticket_is_left_out_when_the_repository_does_not_cite_them() {
        let ctx = context(&["Add login page", "Fix crash"], Some("ABC-3-signup"));
        let prompt = build(&ctx, &files(), &PromptOptions::default());
        assert!(!prompt.system.contains("ABC-3"));
    }

    #[test]
    fn note_goes_last_and_the_diff_makes_room_for_it() {
        let big: Vec<FileDiff> = (0..20)
            .map(|n| FileDiff {
                path: format!("src/m{n}.rs"),
                old_path: None,
                lines: Some((60, 0)),
                hunks: format!(
                    "@@ -0,0 +1,60 @@\n{}",
                    "+let value = compute();\n".repeat(60)
                ),
            })
            .collect();
        let ctx = context(&["feat: add a thing to the parser"], None);
        let options = PromptOptions::default();
        let note = "Your previous reply was rejected. ".repeat(10);

        let plain = build(&ctx, &big, &options);
        let noted = build_with_note(&ctx, &big, &options, Some(&note));
        assert_eq!(noted.system, plain.system);
        assert!(noted.user.ends_with(&format!("\n{note}\n")));
        assert!(noted.user.len() - note.len() < plain.user.len());
        let used = diff::estimate_tokens(&noted.system)
            + diff::estimate_tokens(&noted.user)
            + TEMPLATE_OVERHEAD_TOKENS;
        assert!(used <= options.max_prompt_tokens, "{used}");
    }

    #[test]
    fn large_diff_fits_the_prompt_budget() {
        let big: Vec<FileDiff> = (0..50)
            .map(|n| FileDiff {
                path: format!("src/module_{n}.rs"),
                old_path: None,
                lines: Some((60, 0)),
                hunks: format!(
                    "@@ -0,0 +1,60 @@\n{}",
                    "+let value = compute();\n".repeat(60)
                ),
            })
            .collect();
        let ctx = context(&["feat: add a thing to the parser"], None);
        let options = PromptOptions::default();
        let prompt = build(&ctx, &big, &options);
        let used = diff::estimate_tokens(&prompt.system)
            + diff::estimate_tokens(&prompt.user)
            + TEMPLATE_OVERHEAD_TOKENS;
        assert!(used <= options.max_prompt_tokens, "{used}");
        assert!(prompt.user.contains("partly shown"));
    }
}
