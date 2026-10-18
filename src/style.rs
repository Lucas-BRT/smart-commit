//! Infers the commit message style of a repository from its history.
//!
//! Everything here is deterministic and works on plain commit subjects, so
//! it can be tested without git or an NPU.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Minimum share of subjects that must follow a convention for the
/// repository to be considered as using it.
const CONVENTION_THRESHOLD: f64 = 0.6;
/// Minimum share of subjects citing an issue key for the repository to be
/// considered as citing them. Lower than the convention threshold because
/// chores and merges from main rarely have a ticket.
const TICKET_THRESHOLD: f64 = 0.5;
const MAX_EXAMPLES: usize = 5;
const EXAMPLE_LEN: std::ops::RangeInclusive<usize> = 8..=100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Convention {
    /// No history to learn from.
    Unknown,
    /// <https://www.conventionalcommits.org>: `type(scope)!: description`.
    Conventional,
    /// <https://gitmoji.dev>: subject starts with an emoji or `:shortcode:`.
    Gitmoji,
    Free,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Unknown,
    English,
    Portuguese,
}

/// Whether and how subjects cite an issue key such as `ABC-123`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketStyle {
    None,
    /// The subject, or its description after a convention prefix, starts
    /// with the key: `ABC-123: fix login`, `[ABC-123] fix login`,
    /// `fix: ABC-123 login`.
    Prefix,
    /// The key appears elsewhere in the subject.
    Mention,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub convention: Convention,
    pub language: Language,
    pub tickets: TicketStyle,
    /// Conventional commit types with their counts, most used first.
    pub types: Vec<(String, usize)>,
    /// Conventional commit scopes with their counts, most used first.
    pub scopes: Vec<(String, usize)>,
    /// Recent subjects that represent the style well, most recent first.
    pub examples: Vec<String>,
    /// Number of subjects the style was inferred from.
    pub sample_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionalHeader<'a> {
    pub kind: &'a str,
    pub scope: Option<&'a str>,
    pub breaking: bool,
    pub description: &'a str,
}

/// Infers the style from commit subjects, most recent first.
pub fn detect<S: AsRef<str>>(subjects: &[S]) -> Style {
    let subjects: Vec<&str> = subjects.iter().map(|s| s.as_ref().trim()).collect();
    let convention = detect_convention(&subjects);

    let headers: Vec<ConventionalHeader> = subjects
        .iter()
        .filter_map(|s| parse_conventional(s))
        .collect();
    let types = rank(headers.iter().map(|h| h.kind));
    let scopes = rank(headers.iter().filter_map(|h| h.scope));

    Style {
        convention,
        language: detect_language(&subjects),
        tickets: detect_tickets(&subjects),
        types,
        scopes,
        examples: pick_examples(&subjects, convention),
        sample_size: subjects.len(),
    }
}

/// Parses a conventional commit header: `type(scope)!: description`.
pub fn parse_conventional(subject: &str) -> Option<ConventionalHeader<'_>> {
    let kind_len = subject
        .find(|c: char| !c.is_ascii_lowercase())
        .unwrap_or(subject.len());
    if kind_len == 0 {
        return None;
    }
    let (kind, mut rest) = subject.split_at(kind_len);

    let mut scope = None;
    if let Some(after_paren) = rest.strip_prefix('(') {
        let end = after_paren.find(')')?;
        let inner = after_paren[..end].trim();
        if inner.is_empty() {
            return None;
        }
        scope = Some(inner);
        rest = &after_paren[end + 1..];
    }

    let breaking = rest.starts_with('!');
    if breaking {
        rest = &rest[1..];
    }

    let description = rest.strip_prefix(": ")?.trim();
    if description.is_empty() {
        return None;
    }
    Some(ConventionalHeader {
        kind,
        scope,
        breaking,
        description,
    })
}

/// Whether the subject starts with an emoji or a `:shortcode:`.
pub fn is_gitmoji(subject: &str) -> bool {
    if let Some(rest) = subject.strip_prefix(':') {
        return rest
            .find(':')
            .is_some_and(|end| end > 0 && rest[..end].chars().all(is_shortcode_char));
    }
    subject.chars().next().is_some_and(is_emoji)
}

fn is_shortcode_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '+')
}

/// Covers the blocks gitmoji draws from; not a complete emoji table.
fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF   // pictographs, emoticons, transport, symbols
            | 0x2300..=0x23FF   // misc technical (⏪, ⌛)
            | 0x2600..=0x27BF   // misc symbols, dingbats (♻, ✨, ✅)
            | 0x2B00..=0x2BFF   // arrows (⬆, ⬇)
    )
}

/// Finds a Jira-style issue key (`ABC-123`) delimited by non-alphanumerics.
///
/// Only uppercase keys are matched; lowercase ones are too easy to confuse
/// with words like `fix-2`.
pub fn find_ticket(text: &str) -> Option<&str> {
    ticket_span(text).map(|(start, end)| &text[start..end])
}

/// Byte range of the first issue key in `text`.
fn ticket_span(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    (0..bytes.len()).find_map(|start| {
        let at_boundary = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        if !at_boundary || !bytes[start].is_ascii_uppercase() {
            return None;
        }
        let project_end = start
            + bytes[start..]
                .iter()
                .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                .count();
        if bytes.get(project_end) != Some(&b'-') {
            return None;
        }
        let number_start = project_end + 1;
        let end = number_start
            + bytes[number_start..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        let followed_by_alnum = bytes.get(end).is_some_and(u8::is_ascii_alphanumeric);
        (end > number_start && !followed_by_alnum).then_some((start, end))
    })
}

fn detect_tickets(subjects: &[&str]) -> TicketStyle {
    // Position 1 covers `[ABC-123]` and `(ABC-123)`.
    let starts_with_ticket = |text: &str| ticket_span(text).is_some_and(|(start, _)| start <= 1);
    let citing: Vec<&str> = subjects
        .iter()
        .copied()
        .filter(|s| find_ticket(s).is_some())
        .collect();
    if subjects.is_empty() || (citing.len() as f64 / subjects.len() as f64) < TICKET_THRESHOLD {
        return TicketStyle::None;
    }
    let prefixed = citing
        .iter()
        .filter(|s| starts_with_ticket(s) || starts_with_ticket(description_of(s)))
        .count();
    if prefixed * 2 > citing.len() {
        TicketStyle::Prefix
    } else {
        TicketStyle::Mention
    }
}

fn detect_convention(subjects: &[&str]) -> Convention {
    if subjects.is_empty() {
        return Convention::Unknown;
    }
    let share = |count: usize| count as f64 / subjects.len() as f64;
    let conventional = subjects
        .iter()
        .filter(|s| parse_conventional(s).is_some())
        .count();
    let gitmoji = subjects.iter().filter(|s| is_gitmoji(s)).count();

    if share(conventional) >= CONVENTION_THRESHOLD {
        Convention::Conventional
    } else if share(gitmoji) >= CONVENTION_THRESHOLD {
        Convention::Gitmoji
    } else {
        Convention::Free
    }
}

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

/// The human-written part of a subject, without convention prefixes, so a
/// conventional `fix:` does not count as English.
fn description_of(subject: &str) -> &str {
    if let Some(header) = parse_conventional(subject) {
        return header.description;
    }
    if is_gitmoji(subject) {
        let rest = match subject.strip_prefix(':') {
            Some(rest) => rest.split_once(':').map_or("", |(_, after)| after),
            None => subject.trim_start_matches(|c: char| is_emoji(c) || c == '\u{FE0F}'),
        };
        return rest.trim_start();
    }
    subject
}

fn pick_examples(subjects: &[&str], convention: Convention) -> Vec<String> {
    let mut examples: Vec<String> = Vec::new();
    for subject in subjects {
        if examples.len() == MAX_EXAMPLES {
            break;
        }
        let fits_convention = match convention {
            Convention::Conventional => parse_conventional(subject).is_some(),
            Convention::Gitmoji => is_gitmoji(subject),
            Convention::Free | Convention::Unknown => true,
        };
        if fits_convention
            && !is_noise(subject)
            && EXAMPLE_LEN.contains(&subject.chars().count())
            && !examples.iter().any(|e| e == subject)
        {
            examples.push((*subject).to_owned());
        }
    }
    examples
}

/// Subjects git writes for us (autosquash markers, reverts) and
/// work-in-progress commits teach the model nothing about the style.
fn is_noise(subject: &str) -> bool {
    let lower = subject.to_lowercase();
    let first_word = lower
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty());
    ["fixup!", "squash!", "amend!", "revert \""]
        .iter()
        .any(|p| lower.starts_with(p))
        || first_word == Some("wip")
}

/// Counts occurrences, ordered by count (desc) then name.
fn rank<'a>(items: impl Iterator<Item = &'a str>) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in items {
        *counts.entry(item).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> =
        counts.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_conventional_header() {
        assert_eq!(
            parse_conventional("feat(api)!: drop v1 endpoints"),
            Some(ConventionalHeader {
                kind: "feat",
                scope: Some("api"),
                breaking: true,
                description: "drop v1 endpoints",
            })
        );
    }

    #[test]
    fn parses_header_without_scope() {
        let header = parse_conventional("fix: handle empty input").unwrap();
        assert_eq!((header.kind, header.scope), ("fix", None));
        assert!(!header.breaking);
    }

    #[test]
    fn rejects_non_conventional_subjects() {
        for subject in [
            "Fix: capitalised type",
            "fix:no space",
            "fix: ",
            "fix(): empty scope",
            "fix(api: unclosed scope",
            ": no type",
            "Update README.md",
            "",
        ] {
            assert_eq!(parse_conventional(subject), None, "{subject:?}");
        }
    }

    #[test]
    fn recognises_gitmoji() {
        assert!(is_gitmoji("✨ add login page"));
        assert!(is_gitmoji("🐛 fix crash on empty diff"));
        assert!(is_gitmoji("♻️ refactor parser"));
        assert!(is_gitmoji(":sparkles: add login page"));
        assert!(!is_gitmoji(":: not a shortcode"));
        assert!(!is_gitmoji(":Not_lowercase: nope"));
        assert!(!is_gitmoji("add login page"));
        assert!(!is_gitmoji(""));
    }

    #[test]
    fn empty_history_is_unknown() {
        let style = detect::<&str>(&[]);
        assert_eq!(style.convention, Convention::Unknown);
        assert_eq!(style.language, Language::Unknown);
        assert_eq!(style.sample_size, 0);
        assert!(style.examples.is_empty());
    }

    #[test]
    fn detects_conventional_commits_with_ranked_scopes() {
        let style = detect(&[
            "feat(cli): add --dry-run flag",
            "fix(git): handle detached HEAD",
            "feat(cli): print detected style",
            "docs: describe phase 1",
            "Merge pull request #3 from someone/branch",
        ]);
        assert_eq!(style.convention, Convention::Conventional);
        assert_eq!(
            style.types,
            vec![("feat".into(), 2), ("docs".into(), 1), ("fix".into(), 1)]
        );
        assert_eq!(style.scopes, vec![("cli".into(), 2), ("git".into(), 1)]);
        assert_eq!(
            style.examples.len(),
            4,
            "the merge subject is not an example"
        );
    }

    #[test]
    fn detects_gitmoji() {
        let style = detect(&["✨ add login", "🐛 fix crash", ":memo: update docs", "bump"]);
        assert_eq!(style.convention, Convention::Gitmoji);
        assert_eq!(
            style.examples,
            vec!["✨ add login", "🐛 fix crash", ":memo: update docs"]
        );
    }

    #[test]
    fn mixed_history_below_threshold_is_free() {
        let style = detect(&["feat: one", "fix: two", "Update README", "Initial commit"]);
        assert_eq!(style.convention, Convention::Free);
    }

    #[test]
    fn detects_portuguese_ignoring_conventional_prefix() {
        let style = detect(&[
            "fix: corrige o parser de diff",
            "feat: adiciona suporte para gitmoji",
            "docs: atualiza a documentação da fase 1",
        ]);
        assert_eq!(style.language, Language::Portuguese);
    }

    #[test]
    fn detects_english() {
        let style = detect(&[
            "Add support for renames",
            "Fix crash when the diff is empty",
        ]);
        assert_eq!(style.language, Language::English);
    }

    #[test]
    fn type_words_alone_do_not_make_english() {
        let style = detect(&["fix: parser", "feat: gitmoji"]);
        assert_eq!(style.language, Language::Unknown);
    }

    #[test]
    fn gitmoji_prefix_is_stripped_for_language() {
        assert_eq!(description_of(":bug: corrige crash"), "corrige crash");
        assert_eq!(description_of("♻️ refatora parser"), "refatora parser");
    }

    #[test]
    fn detects_ticket_prefix() {
        let style = detect(&[
            "ABC-12: fix login",
            "[ABC-13] add logout",
            "chore: bump deps",
        ]);
        assert_eq!(style.tickets, TicketStyle::Prefix);
    }

    #[test]
    fn ticket_after_convention_prefix_is_a_prefix() {
        let gitmoji = detect(&["✨ ABC-1 add login page", "🐛 ABC-2 fix crash"]);
        assert_eq!(gitmoji.tickets, TicketStyle::Prefix);
        let conventional = detect(&["feat: [ABC-1] add login", "fix(api): ABC-2 crash"]);
        assert_eq!(conventional.tickets, TicketStyle::Prefix);
    }

    #[test]
    fn detects_ticket_mention() {
        let style = detect(&["fix login (ABC-12)", "add logout, refs OPS-1"]);
        assert_eq!(style.tickets, TicketStyle::Mention);
    }

    #[test]
    fn occasional_tickets_are_ignored() {
        let style = detect(&["ABC-12: fix login", "add logout", "bump deps"]);
        assert_eq!(style.tickets, TicketStyle::None);
        assert_eq!(detect::<&str>(&[]).tickets, TicketStyle::None);
    }

    #[test]
    fn recognises_noise_subjects() {
        for subject in [
            "fixup! feat: add flag",
            "squash! fix parser",
            "Revert \"feat: add flag\"",
            "WIP",
            "wip: half done",
            "[WIP] login page",
        ] {
            assert!(is_noise(subject), "{subject:?}");
        }
        for subject in [
            "wipe stale cache",
            "Revert to the old parser",
            "feat: add flag",
        ] {
            assert!(!is_noise(subject), "{subject:?}");
        }
    }

    #[test]
    fn examples_skip_noise_duplicates_and_bad_lengths() {
        let long = format!("feat: {}", "x".repeat(120));
        let style = detect(&[
            "fixup! feat: add flag",
            "WIP: half done",
            "feat: add flag",
            "feat: add flag",
            "fix: ok",
            long.as_str(),
            "feat: one",
            "feat: two thing",
            "feat: three thing",
            "feat: four thing",
            "feat: five thing",
        ]);
        assert_eq!(
            style.examples,
            vec![
                "feat: add flag",
                "feat: one",
                "feat: two thing",
                "feat: three thing",
                "feat: four thing",
            ]
        );
    }
}
