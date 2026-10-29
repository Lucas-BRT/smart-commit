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

/// Whether a subject can show the repository's style: it follows the
/// convention, is no noise (merges, version bumps) and is neither too short
/// nor too long.
fn fits_example(subject: &str, convention: Convention) -> bool {
    let fits_convention = match convention {
        Convention::Conventional => parse_conventional(subject).is_some(),
        Convention::Gitmoji => is_gitmoji(subject),
        Convention::Free | Convention::Unknown => true,
    };
    fits_convention && !is_noise(subject) && EXAMPLE_LEN.contains(&subject.chars().count())
}

fn pick_examples(subjects: &[&str], convention: Convention) -> Vec<String> {
    let mut examples: Vec<String> = Vec::new();
    for subject in subjects {
        if examples.len() == MAX_EXAMPLES {
            break;
        }
        if fits_example(subject, convention) && !examples.iter().any(|e| e == subject) {
            examples.push((*subject).to_owned());
        }
    }
    examples
}

/// How close the files of a past commit are to the staged ones: for each
/// staged path, 3 if the commit touched it, else 2 if it touched a file of
/// its directory, else 1 if one of its top-level directory.
fn closeness(staged: &[&str], past: &[String]) -> usize {
    let top = |path: &str| path.split('/').next().unwrap_or_default().to_owned();
    staged
        .iter()
        .map(|s| {
            past.iter()
                .map(|f| {
                    if f == s {
                        3
                    } else if area(f) == area(s) {
                        2
                    } else if f.contains('/') && s.contains('/') && top(f) == top(s) {
                        1
                    } else {
                        0
                    }
                })
                .max()
                .unwrap_or(0)
        })
        .sum()
}

/// The examples for a change to the `staged` paths, in the order of the
/// history: up to `similar` of the commits that touched the closest files
/// (those that touched none are not "similar"), completed with the most
/// recent ones to as many as [`pick_examples`] gives. `files` holds the
/// paths of each of `subjects`; without them (traces written before they
/// were collected) it is the recent ones.
pub fn similar_examples(
    subjects: &[String],
    files: &[Vec<String>],
    staged: &[&str],
    convention: Convention,
    similar: usize,
) -> Vec<String> {
    let recent: Vec<&str> = subjects.iter().map(String::as_str).collect();
    if files.len() != subjects.len() {
        return pick_examples(&recent, convention);
    }
    let mut eligible: Vec<usize> = Vec::new();
    for (i, subject) in subjects.iter().enumerate() {
        if fits_example(subject, convention) && !eligible.iter().any(|&j| subjects[j] == *subject) {
            eligible.push(i);
        }
    }
    let mut ranked: Vec<(usize, usize)> = eligible
        .iter()
        .map(|&i| (closeness(staged, &files[i]), i))
        .filter(|&(score, _)| score > 0)
        .collect();
    // Closest first, the more recent among equals.
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut chosen: Vec<usize> = ranked
        .into_iter()
        .take(similar.min(MAX_EXAMPLES))
        .map(|(_, i)| i)
        .collect();
    for &i in &eligible {
        if chosen.len() == MAX_EXAMPLES {
            break;
        }
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    chosen.sort_unstable();
    chosen.into_iter().map(|i| subjects[i].clone()).collect()
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
