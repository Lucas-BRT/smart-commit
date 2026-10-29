//! Turns the repository context and the staged diff into a chat prompt.
//!
//! Instructions are written in English, which small models follow best,
//! and ask for the message in the repository's own language.

mod system;
mod types;
mod user;

#[cfg(test)]
mod tests;

pub use system::listed_types;
pub use types::{
    DEFAULT_MAX_PROMPT_TOKENS, DiffFormat, Prompt, PromptExamples, PromptOptions, PromptRules,
    PromptScopes,
};

use crate::context::RepoContext;
use crate::diff::{self, CompactDiff, FileDiff};
use crate::style::{Convention, Language, Style};

use self::types::TEMPLATE_OVERHEAD_TOKENS;

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
    let system = system::system_message(ctx, options);
    // The scope map is extra: the diff is sized as if the scopes were only
    // listed, so a model trained with the map sees as much of it as one
    // trained without.
    let sized = match options.scopes {
        PromptScopes::List => None,
        PromptScopes::Map => Some(system::system_message(
            ctx,
            &PromptOptions {
                scopes: PromptScopes::List,
                ..options.clone()
            },
        )),
    };
    // Sized with the truncation note at its longest, which large diffs
    // will need.
    let empty = CompactDiff {
        truncated: usize::MAX,
        ..CompactDiff::default()
    };
    let fixed = diff::estimate_tokens(sized.as_ref().unwrap_or(&system))
        + diff::estimate_tokens(&user::user_message(&empty, note))
        + TEMPLATE_OVERHEAD_TOKENS;
    let budget = options.max_prompt_tokens.saturating_sub(fixed);
    let compacted = match options.diff {
        DiffFormat::Plain => diff::compact(files, budget),
        DiffFormat::Annotated => diff::compact_annotated(files, &ctx.staged, budget),
    };
    let user = user::user_message(&compacted, note);
    Prompt { system, user }
}
