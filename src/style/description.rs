//! The words of a commit subject's description.

use super::{is_gitmoji, parse_conventional};

/// The lowercase words of a subject's description.
pub fn words(subject: &str) -> Vec<String> {
    description_of(subject)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The human-written part of a subject, without convention prefixes, so a
/// conventional `fix:` does not count as a word of the description.
pub fn description_of(subject: &str) -> &str {
    if let Some(header) = parse_conventional(subject) {
        return header.description;
    }
    if is_gitmoji(subject) {
        let rest = match subject.strip_prefix(':') {
            Some(rest) => rest.split_once(':').map_or("", |(_, after)| after),
            None => subject
                .trim_start_matches(|c: char| super::gitmoji::is_emoji_char(c) || c == '\u{FE0F}'),
        };
        return rest.trim_start();
    }
    subject
}
