//! Parsing Conventional Commits headers.

use super::ConventionalHeader;

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
