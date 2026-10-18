//! Turns the repository context and the staged diff into a chat prompt.
//!
//! Instructions are written in English, which small models follow best,
//! and ask for the message in the repository's own language.

use std::fmt::Write as _;

use crate::context::RepoContext;
use crate::diff::{self, CompactDiff, FileDiff};
use crate::style::{Convention, Language, Style, TicketStyle};

/// The NPU pipeline's default `MAX_PROMPT_LEN`.
pub const DEFAULT_MAX_PROMPT_TOKENS: usize = 1024;
/// Room for the chat template's role markers around our two messages.
const TEMPLATE_OVERHEAD_TOKENS: usize = 40;
/// Types and scopes beyond these are rarely used and only cost tokens.
const MAX_LISTED: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptOptions {
    pub max_prompt_tokens: usize,
    /// Used when the history is too short or mixed to tell.
    pub default_convention: Convention,
    pub default_language: Language,
}

impl Default for PromptOptions {
    fn default() -> Self {
        Self {
            max_prompt_tokens: DEFAULT_MAX_PROMPT_TOKENS,
            default_convention: Convention::Conventional,
            default_language: Language::English,
        }
    }
}

/// The convention and language the message must follow, after defaults.
pub fn effective_style(style: &Style, options: &PromptOptions) -> (Convention, Language) {
    let convention = match style.convention {
        Convention::Unknown => options.default_convention,
        known => known,
    };
    let language = match style.language {
        Language::Unknown => options.default_language,
        known => known,
    };
    (convention, language)
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
    let user = user_message(&diff::compact(files, budget), note);
    Prompt { system, user }
}

fn system_message(ctx: &RepoContext, options: &PromptOptions) -> String {
    let style = &ctx.style;
    let (convention, language) = effective_style(style, options);
    let mut text = String::from(
        "You write git commit messages from a diff. Reply with the commit \
         message only: no code fences, no quotes, no explanation.\n\nRules:\n",
    );
    let mut rule = |line: &str| {
        let _ = writeln!(text, "- {line}");
    };

    rule("Subject line of at most 72 characters, without a trailing period.");
    rule(match language {
        Language::Portuguese => "Write in Brazilian Portuguese.",
        _ => "Write in English, in the imperative mood (\"Add\", not \"Added\").",
    });
    match convention {
        Convention::Conventional => {
            rule("Use Conventional Commits: `type(scope): description`.");
            if style.types.is_empty() {
                rule("Types: feat, fix, docs, refactor, test, chore.");
            } else {
                rule(&format!("Types used here: {}.", names(&style.types)));
            }
            if !style.scopes.is_empty() {
                rule(&format!(
                    "Scopes used here: {}. Omit the scope if none fits.",
                    names(&style.scopes)
                ));
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
    rule("Add a body only if the reason for the change is not obvious.");

    if !style.examples.is_empty() {
        text.push_str("\nRecent subjects from this repository, follow their style:\n");
        for example in &style.examples {
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
        RepoContext {
            root: PathBuf::from("/repo"),
            branch: branch.map(str::to_owned),
            ticket: branch.and_then(crate::context::ticket_from_branch),
            style: style::detect(subjects),
            staged: Vec::new(),
        }
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
