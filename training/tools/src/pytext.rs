//! The bits of Python's `str` the tools relied on, where Rust's differ.

use std::path::Path;

use anyhow::{Context, Result};

/// The lines of a JSON Lines file, split at `\n` only (the scripts read them
/// with `text.split("\n")`): a `\u{2028}` or a `\r` inside a line stays in
/// it, and the empty piece after the last newline is not a line.
pub fn read_lines(path: &Path) -> Result<Vec<String>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(lines)
}

/// `str.isspace` for one character: Rust's White_Space plus the separators
/// `\x1c` to `\x1f`.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.strip()`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `str.splitlines()`: it breaks at more than `\n`.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let breaks = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !breaks {
            continue;
        }
        lines.push(&text[start..i]);
        start = i + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|&(_, next)| next == '\n') {
            chars.next();
            start += 1;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_breaks_where_python_does() {
        assert_eq!(
            splitlines("a\nb\r\nc\rd\u{2028}e"),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(splitlines("a\n"), ["a"]);
        assert_eq!(splitlines("a\n\nb"), ["a", "", "b"]);
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(splitlines("\n"), [""]);
    }

    #[test]
    fn read_lines_splits_at_newlines_only() {
        let dir = tempfile::tempdir().unwrap();
        let read = |text: &str| {
            let path = dir.path().join("f.jsonl");
            std::fs::write(&path, text).unwrap();
            read_lines(&path).unwrap()
        };
        assert_eq!(read("a\nb\n"), ["a", "b"]);
        assert_eq!(read("a\nb"), ["a", "b"]);
        assert_eq!(read("a\n\nb\n"), ["a", "", "b"]);
        assert_eq!(read("a\u{2028}b\rc\n"), ["a\u{2028}b\rc"]);
        assert_eq!(read(""), Vec::<String>::new());
        assert_eq!(read("\n"), [""]);
        assert!(read_lines(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn strip_removes_the_separators_python_counts_as_space() {
        assert_eq!(strip(" \u{1c}x\u{a0}\n"), "x");
        assert_eq!(strip("\u{200b}x"), "\u{200b}x");
    }
}
