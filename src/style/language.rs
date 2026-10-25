//! Language detection for commit messages.

use super::{Language, is_gitmoji, parse_conventional};

// Ambiguous words ("a", "no", "remove") are left out on purpose.
const PORTUGUESE_WORDS: &[&str] = &[
    "de",
    "da",
    "do",
    "das",
    "dos",
    "para",
    "com",
    "em",
    "na",
    "nas",
    "nos",
    "não",
    "nao",
    "e",
    "o",
    "os",
    "as",
    "um",
    "uma",
    "ao",
    "adiciona",
    "adicionar",
    "adicionado",
    "corrige",
    "corrigir",
    "correção",
    "correcao",
    "atualiza",
    "atualizar",
    "ajusta",
    "ajuste",
    "melhora",
    "cria",
    "criar",
    "implementa",
    "remove",
    "removido",
    "versão",
    "versao",
    "configuração",
    "arquivo",
    "testes",
    "quando",
];
const ENGLISH_WORDS: &[&str] = &[
    "the",
    "to",
    "for",
    "with",
    "and",
    "of",
    "in",
    "on",
    "from",
    "into",
    "when",
    "add",
    "adds",
    "added",
    "fix",
    "fixes",
    "fixed",
    "update",
    "updates",
    "updated",
    "use",
    "implement",
    "improve",
    "create",
    "removed",
    "tests",
    "file",
    "version",
    "support",
];

fn detect_language(subjects: &[&str]) -> Language {
    let (mut pt, mut en) = (0usize, 0usize);
    for subject in subjects {
        for word in description_of(subject)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
        {
            pt += usize::from(PORTUGUESE_WORDS.contains(&word.as_str()));
            en += usize::from(ENGLISH_WORDS.contains(&word.as_str()));
        }
    }
    match pt.cmp(&en) {
        std::cmp::Ordering::Greater if pt >= 2 => Language::Portuguese,
        std::cmp::Ordering::Less if en >= 2 => Language::English,
        _ => Language::Unknown,
    }
}

/// Words of [`PORTUGUESE_WORDS`] that English subjects use too ("remove
/// unused exports", "use X as Y"): harmless summed over a history,
/// misleading in one subject. "do" stays Portuguese: "fix do teclado" is
/// how Portuguese repositories write, and "do not ..." mixes with English
/// words anyway.
const AMBIGUOUS_ALONE: &[&str] = &["remove", "as", "e", "o", "os"];

/// The language of one subject, from the same words as the history's:
/// known only when it has words of one language and none of the other, so
/// a subject mixing them, as many repositories do, is unknown.
pub fn language_of(subject: &str) -> Language {
    let (mut pt, mut en) = (0usize, 0usize);
    for word in words(subject) {
        if AMBIGUOUS_ALONE.contains(&word.as_str()) {
            continue;
        }
        pt += usize::from(PORTUGUESE_WORDS.contains(&word.as_str()));
        en += usize::from(ENGLISH_WORDS.contains(&word.as_str()));
    }
    match (pt, en) {
        (0, 0) => Language::Unknown,
        (_, 0) => Language::Portuguese,
        (0, _) => Language::English,
        _ => Language::Unknown,
    }
}

/// The lowercase words of a subject's description.
pub fn words(subject: &str) -> Vec<String> {
    description_of(subject)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The human-written part of a subject, without convention prefixes, so a
/// conventional `fix:` does not count as English.
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

pub(crate) fn detect_language_for_subjects(subjects: &[&str]) -> Language {
    detect_language(subjects)
}
