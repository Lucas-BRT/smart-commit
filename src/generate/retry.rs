//! Retry logic for generation.

use crate::message::CommitMessage;
use crate::style::{self, Language};

/// The share of an earlier subject's words a reply must cover, with no
/// word of its own, to count as a copy of it.
const NEAR_COPY_SHARE: f64 = 0.6;

/// Words no subject ends with: a reply that stops after one was cut off.
/// `in`, `on`, `up`, `off` and `out` end real subjects ("log in", "turn
/// off"), and so do pronouns, so they are not here.
const DANGLING_ENGLISH: &[&str] = &[
    "the", "a", "an", "of", "to", "for", "with", "from", "by", "and", "or", "into", "than", "that",
    "which", "as", "at",
];
const DANGLING_PORTUGUESE: &[&str] = &[
    "o", "a", "os", "as", "um", "uma", "de", "do", "da", "dos", "das", "em", "no", "na", "nos",
    "nas", "para", "com", "por", "e", "ou", "que", "ao", "aos",
];

/// Git's prepared message as subject and body.
pub fn prepared_message(text: &str) -> CommitMessage {
    let (subject, body) = text.split_once('\n').unwrap_or((text, ""));
    let body = body.trim();
    CommitMessage {
        subject: subject.trim().to_owned(),
        body: (!body.is_empty()).then(|| body.to_owned()),
    }
}

/// The history subject `subject` repeats, ignoring case and a final period.
///
/// Or nearly: every word of the subject's description, four or more, is in
/// a history subject's, and covers at least [`NEAR_COPY_SHARE`] of it, as
/// when the model drops a few words of an earlier subject. A subject that
/// adds words of its own (a history subject plus "in hooks") is not a copy.
pub fn copied_from<'a>(subject: &str, history: &'a [String]) -> Option<&'a str> {
    let normalize = |s: &str| s.trim().trim_end_matches('.').to_lowercase();
    let exact = normalize(subject);
    if let Some(h) = history.iter().find(|h| normalize(h) == exact) {
        return Some(h);
    }
    let words = style::words(subject);
    if words.len() < 4 {
        return None;
    }
    history
        .iter()
        .find(|h| {
            let theirs = style::words(h);
            words.iter().all(|w| theirs.contains(w))
                && words.len() as f64 >= NEAR_COPY_SHARE * theirs.len() as f64
        })
        .map(String::as_str)
}

/// Whether `subject` is clearly in another language than `language`, the
/// repository's: only when its words are all of the other one, and no
/// history subject is in that one (a repository that sometimes writes it
/// accepts it).
pub fn in_other_language(subject: &str, language: Language, history: &[String]) -> bool {
    let theirs = style::language_of(subject);
    matches!(
        (language, theirs),
        (Language::Portuguese, Language::English) | (Language::English, Language::Portuguese)
    ) && !history.iter().any(|h| style::language_of(h) == theirs)
}

/// Whether `subject` stops in the middle of a sentence: its last word is an
/// article, a preposition or a conjunction of `language` (or of English,
/// which commits mix in). A word in backticks or quotes, or with a digit, is
/// a name and not one of them.
pub fn cut_off(subject: &str, language: Language) -> bool {
    let description = subject
        .split_once(':')
        .filter(|(head, _)| !head.contains(' '))
        .map_or(subject, |(_, rest)| rest);
    let Some(last) = description.split_whitespace().last() else {
        return false;
    };
    if !last.chars().all(char::is_alphabetic) {
        return false;
    }
    let last = last.to_lowercase();
    DANGLING_ENGLISH.contains(&last.as_str())
        || (language == Language::Portuguese && DANGLING_PORTUGUESE.contains(&last.as_str()))
}
