//! Truncation, wrapping and Conventional Commits normalization.

use crate::style;

use super::SanitizeError;

/// Where a long subject can end and still read as a whole, by preference:
/// a clause, then a phrase a preposition opens, then a list.
const BOUNDARIES: &[&[&str]] = &[
    &[", ", "; ", " ("],
    &[
        " to ",
        " for ",
        " in ",
        " on ",
        " with ",
        " when ",
        " by ",
        " from ",
        " so ",
        " after ",
        " before ",
        " because ",
        " without ",
        " via ",
        " using ",
        " instead of ",
    ],
    &[" and "],
];

/// What a cut subject should not end with.
const TRAILING: [char; 5] = [' ', ',', ';', ':', '-'];

/// Fits a subject in `max` columns, without an ellipsis (git
/// tooling shows the subject as is): first a scope that is a file path
/// becomes the file's name (`fix(src/operator/combineLatest.ts)` →
/// `fix(combineLatest)`), then the subject ends at the last boundary of
/// [`BOUNDARIES`] that keeps at least half of the limit, else at the last
/// word that fits.
pub fn truncate_subject(subject: &str, max: usize) -> String {
    if subject.chars().count() <= max {
        return subject.to_owned();
    }
    let subject = shorten_path_scope(subject);
    if subject.chars().count() <= max {
        return subject;
    }
    // Byte offset of the first character past the limit.
    let limit = subject
        .char_indices()
        .nth(max)
        .map_or(subject.len(), |(i, _)| i);
    let shortest = subject
        .char_indices()
        .nth(max / 2)
        .map_or(subject.len(), |(i, _)| i);
    for group in BOUNDARIES {
        let end = group
            .iter()
            .filter_map(|b| subject[..limit].rfind(b))
            .filter(|&at| at >= shortest)
            .max();
        if let Some(end) = end {
            return subject[..end].trim_end_matches(TRAILING).to_owned();
        }
    }
    let cut: String = subject.chars().take(max + 1).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > 0 => &cut[..space],
        _ => &cut[..limit.min(cut.len())],
    };
    cut.trim_end_matches(TRAILING).to_owned()
}

/// `type(path/to/file.ext): ...` as `type(file): ...`; models put the path
/// of the file they read in the scope, which repositories do not. A scope
/// without an extension (`provider/kubernetes`) is a real one: kept.
fn shorten_path_scope(subject: &str) -> String {
    let Some(header) = style::parse_conventional(subject) else {
        return subject.to_owned();
    };
    let Some(scope) = header.scope else {
        return subject.to_owned();
    };
    let first = scope.split(',').next().unwrap_or(scope).trim();
    let name = first.rsplit('/').next().unwrap_or(first);
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return subject.to_owned();
    };
    if stem.is_empty() || extension.is_empty() || !extension.chars().all(char::is_alphanumeric) {
        return subject.to_owned();
    }
    let bang = if header.breaking { "!" } else { "" };
    format!("{}({stem}){bang}: {}", header.kind, header.description)
}

/// Accepts the common near misses: `Feat: x`, `feat:x`, `feat : x`.
pub fn fix_conventional(subject: &str) -> Result<String, SanitizeError> {
    if style::parse_conventional(subject).is_some() {
        return Ok(subject.to_owned());
    }
    let not_conventional = || SanitizeError::NotConventional(subject.to_owned());
    let (head, description) = subject.split_once(':').ok_or_else(not_conventional)?;
    let head = head.trim();
    let kind_len = head.find(['(', '!']).unwrap_or(head.len());
    let candidate = format!(
        "{}{}: {}",
        head[..kind_len].to_lowercase(),
        &head[kind_len..],
        description.trim()
    );
    if style::parse_conventional(&candidate).is_some() {
        Ok(candidate)
    } else {
        Err(not_conventional())
    }
}

/// Re-wraps paragraphs and `- ` bullets at [`BODY_WIDTH`].
pub fn wrap_body(lines: &[&str]) -> String {
    // Each block is (bullet?, words); blank lines end paragraphs.
    let mut blocks: Vec<(bool, Vec<&str>)> = Vec::new();
    let mut open = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        let bullet = ["- ", "* ", "• "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m));
        match (bullet, open) {
            (Some(item), _) => blocks.push((true, item.split_whitespace().collect())),
            (None, true) => blocks
                .last_mut()
                .expect("open implies a block")
                .1
                .extend(trimmed.split_whitespace()),
            (None, false) => blocks.push((false, trimmed.split_whitespace().collect())),
        }
        open = true;
    }

    let mut out: Vec<String> = Vec::new();
    for (i, (bullet, words)) in blocks.iter().enumerate() {
        let after_paragraph = i > 0 && !(blocks[i - 1].0 && *bullet);
        if after_paragraph {
            out.push(String::new());
        }
        let (first, rest) = if *bullet { ("- ", "  ") } else { ("", "") };
        let mut line = first.to_owned();
        let mut empty = true;
        for word in words {
            if !empty && line.chars().count() + 1 + word.chars().count() > super::BODY_WIDTH {
                out.push(std::mem::replace(&mut line, rest.to_owned()));
                empty = true;
            }
            if !empty {
                line.push(' ');
            }
            line.push_str(word);
            empty = false;
        }
        out.push(line);
    }
    out.join("\n")
}
