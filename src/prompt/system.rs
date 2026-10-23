//! System message construction.

use std::fmt::Write as _;

use crate::context::RepoContext;
use crate::message;
use crate::style::{self, Convention, Language, Style, TicketStyle};

use super::effective_style;
use super::types::{MAX_LISTED, PromptOptions};

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

pub(super) fn system_message(ctx: &RepoContext, options: &PromptOptions) -> String {
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

    // A model trained without the rules learned 72 columns from the data:
    // another limit is told even then.
    if full || options.subject_max != message::SUBJECT_MAX {
        rule(&format!(
            "Subject line of at most {} characters, without a trailing period.",
            options.subject_max
        ));
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
                let mut types: Vec<String> = style
                    .types
                    .iter()
                    .take(MAX_LISTED)
                    .map(|(name, _)| name.clone())
                    .collect();
                if !types.iter().any(|t| t == "refactor") {
                    types.push("refactor".to_string());
                }
                if types.len() > MAX_LISTED {
                    types.truncate(MAX_LISTED);
                }
                rule(&format!("Types used here: {}.", types.join(", ")));
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

fn names(ranked: &[(String, usize)]) -> String {
    ranked
        .iter()
        .take(MAX_LISTED)
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
