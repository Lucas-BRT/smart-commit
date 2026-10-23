//! Style detection from commit history.

use std::collections::HashMap;

use super::gitmoji::is_gitmoji;
use super::language::detect_language_for_subjects;
use super::parse::parse_conventional;
use super::ticket::detect_tickets_for_subjects;
use super::{Convention, Style};

/// Minimum share of subjects that must follow a convention for the
/// repository to be considered as using it.
const CONVENTION_THRESHOLD: f64 = 0.6;
const MAX_EXAMPLES: usize = 5;
const EXAMPLE_LEN: std::ops::RangeInclusive<usize> = 8..=100;

/// Infers the style from commit subjects, most recent first.
pub fn detect<S: AsRef<str>>(subjects: &[S]) -> Style {
    let subjects: Vec<&str> = subjects.iter().map(|s| s.as_ref().trim()).collect();
    let convention = detect_convention(&subjects);

    let headers: Vec<super::ConventionalHeader> = subjects
        .iter()
        .filter_map(|s| parse_conventional(s))
        .collect();
    let types = rank(headers.iter().map(|h| h.kind));
    let scopes = rank(headers.iter().filter_map(|h| h.scope));

    Style {
        convention,
        language: detect_language_for_subjects(&subjects),
        tickets: detect_tickets_for_subjects(&subjects),
        types,
        scopes,
        examples: pick_examples(&subjects, convention),
        sample_size: subjects.len(),
    }
}

/// Where a scope's commits went: the areas (a file's directory, or the file
/// itself at the root) of the files touched by the commits using `scope`,
/// most frequent first, at most `max`. `files` holds the paths of each of
/// `subjects`.
pub fn scope_areas<S: AsRef<str>>(
    scope: &str,
    subjects: &[S],
    files: &[Vec<String>],
    max: usize,
) -> Vec<String> {
    let areas = subjects
        .iter()
        .zip(files)
        .filter(|(subject, _)| {
            parse_conventional(subject.as_ref().trim()).and_then(|h| h.scope) == Some(scope)
        })
        .flat_map(|(_, paths)| {
            let mut areas: Vec<&str> = paths.iter().map(|p| area(p)).collect();
            // Counted once per commit, so one wide commit does not decide.
            areas.sort_unstable();
            areas.dedup();
            areas
        });
    rank(areas).into_iter().take(max).map(|(a, _)| a).collect()
}

/// `src/diff.rs` -> `src/`; `mise.toml` -> `mise.toml`.
fn area(path: &str) -> &str {
    path.rfind('/').map_or(path, |slash| &path[..=slash])
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
pub fn is_noise(subject: &str) -> bool {
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
