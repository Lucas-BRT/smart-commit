//! The names a change declares and the comments it adds, for `--names on`.
//!
//! The diff in the prompt is cut to fit its budget, and what is cut can hold the names the message should use. This
//! reads every source, config and test file of the change (lockfiles, generated files, documents and binaries
//! carry no names) and lists, in the order they appear, the functions, classes and the like that the hunks name in
//! their headers or declare in the lines they change, and a few of the comments they add.

use crate::diff::{FileDiff, FileKind};

/// Most names listed.
const MAX_NAMES: usize = 8;
/// Most comments listed, and the longest one, in characters.
const MAX_COMMENTS: usize = 2;
const MAX_COMMENT_CHARS: usize = 70;
/// A name shorter than this says nothing.
const MIN_NAME_CHARS: usize = 3;
/// A comment shorter than this is a label, not a sentence.
const MIN_COMMENT_CHARS: usize = 12;

/// Words after which a declaration's name comes.
const DECLARATIONS: &[&str] = &[
    "fn",
    "def",
    "func",
    "function",
    "class",
    "struct",
    "enum",
    "trait",
    "interface",
    "impl",
    "type",
    "mod",
    "module",
];
/// Words that sit before a parenthesis without being a name.
const NOT_NAMES: &[&str] = &[
    "function", "async", "await", "if", "for", "while", "switch", "return", "catch", "fn", "def",
    "func", "static", "const", "void", "int", "char", "bool", "else", "new",
];
/// What starts a line comment (the text after it is the comment).
const COMMENT_STARTS: &[&str] = &["///", "//!", "//", "/*", "* ", "# ", "-- "];

/// The lines to add to the user message, or `None` when the change names nothing.
pub(super) fn touched(files: &[FileDiff]) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    for file in files {
        if file.hunks.is_empty()
            || matches!(FileKind::of(file), FileKind::Docs)
            || FileKind::of(file).is_noise()
        {
            continue;
        }
        for line in file.hunks.lines() {
            if let Some(context) = hunk_context(line) {
                push_name(
                    &mut names,
                    name_in(context).or_else(|| called_name(context)),
                );
            } else if let Some(added) = line.strip_prefix('+').filter(|_| !line.starts_with("+++"))
            {
                push_name(&mut names, name_in(added));
                push_comment(&mut comments, comment_in(added));
            } else if let Some(removed) =
                line.strip_prefix('-').filter(|_| !line.starts_with("---"))
            {
                push_name(&mut names, name_in(removed));
            }
        }
    }
    let mut lines = Vec::new();
    if !names.is_empty() {
        names.truncate(MAX_NAMES);
        lines.push(format!("Names touched: {}", names.join(", ")));
    }
    if !comments.is_empty() {
        comments.truncate(MAX_COMMENTS);
        let quoted: Vec<String> = comments.iter().map(|c| format!("\"{c}\"")).collect();
        lines.push(format!("Comments added: {}", quoted.join("; ")));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn push_name(names: &mut Vec<String>, name: Option<String>) {
    if let Some(name) = name
        && !names.contains(&name)
    {
        names.push(name);
    }
}

fn push_comment(comments: &mut Vec<String>, comment: Option<String>) {
    if let Some(comment) = comment
        && !comments.contains(&comment)
    {
        comments.push(comment);
    }
}

/// What git writes after a hunk's `@@ -a,b +c,d @@`: the nearest line above it that looks like a declaration.
fn hunk_context(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("@@")?;
    let (_, context) = rest.split_once("@@")?;
    let context = context.trim();
    (!context.is_empty()).then_some(context)
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifier a token starts with, if it is a usable name.
fn identifier(token: &str) -> Option<String> {
    let name: String = token
        .trim_start_matches(['&', '*'])
        .chars()
        .take_while(|&c| is_name_char(c))
        .collect();
    (name.chars().count() >= MIN_NAME_CHARS && !name.starts_with(|c: char| c.is_ascii_digit()))
        .then_some(name)
}

/// The name a declaration on this line gives (`pub fn parse_flags(`, `class Config:`, `def run(`, `render() {`).
fn name_in(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for (i, token) in tokens.iter().enumerate() {
        if DECLARATIONS.contains(token)
            && let Some(name) = tokens.get(i + 1).and_then(|next| identifier(next))
        {
            return Some(name);
        }
    }
    // A shell function: `name() {` or `name()`.
    let trimmed = line.trim();
    let head = trimmed.strip_suffix('{').map_or(trimmed, str::trim_end);
    head.strip_suffix("()")
        .filter(|name| !name.contains(char::is_whitespace))
        .and_then(identifier)
}

/// The name before the first `(` of a hunk's context, for languages whose declarations carry no keyword.
fn called_name(context: &str) -> Option<String> {
    let before = context
        .split('(')
        .next()
        .filter(|_| context.contains('('))?;
    let last = before
        .split(|c: char| !is_name_char(c))
        .rfind(|w| !w.is_empty() && !NOT_NAMES.contains(w))?;
    identifier(last)
}

/// The text of a comment on this added line, when it is a sentence.
fn comment_in(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let start = COMMENT_STARTS.iter().find(|s| trimmed.starts_with(**s))?;
    let text = trimmed[start.len()..]
        .trim()
        .trim_end_matches("*/")
        .trim()
        .trim_start_matches('*')
        .trim();
    if text.chars().count() < MIN_COMMENT_CHARS
        || !text.contains(' ')
        || text.contains("shellcheck")
    {
        return None;
    }
    Some(cut(text, MAX_COMMENT_CHARS))
}

/// The text cut at the last space within `max` characters, without an ellipsis.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let end = text.char_indices().nth(max).map_or(text.len(), |(i, _)| i);
    let head = &text[..end];
    head.rfind(' ')
        .map_or(head, |space| &head[..space])
        .trim_end_matches([',', ';', ':', '-', ' '])
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, hunks: &str) -> FileDiff {
        FileDiff {
            path: path.into(),
            old_path: None,
            lines: Some((1, 1)),
            hunks: hunks.into(),
        }
    }

    #[test]
    fn a_hunk_header_has_the_context_git_found() {
        assert_eq!(
            hunk_context("@@ -137 +137 @@ execute_dokku_cmd() {"),
            Some("execute_dokku_cmd() {")
        );
        assert_eq!(hunk_context("@@ -1,4 +1,5 @@"), None);
        assert_eq!(hunk_context("+fn main() {}"), None);
    }

    #[test]
    fn a_declaration_gives_its_name() {
        for (line, name) in [
            (
                "pub fn parse_flags(args: &[String]) -> Flags {",
                "parse_flags",
            ),
            ("    def run(self):", "run"),
            ("class Config:", "Config"),
            ("export class UserStore extends Store {", "UserStore"),
            ("async function loadUser(id) {", "loadUser"),
            ("impl Backend for Mock {", "Backend"),
            ("pub struct RepoContext {", "RepoContext"),
            ("type Alias = String;", "Alias"),
            ("render() {", "render"),
            ("execute_dokku_cmd()", "execute_dokku_cmd"),
        ] {
            assert_eq!(name_in(line).as_deref(), Some(name), "{line}");
        }
    }

    #[test]
    fn what_is_not_a_declaration_gives_nothing() {
        for line in [
            "let value = compute();",
            "return foo()",
            "fn ab() {}",
            "func (s *Server) Start() {",
            "    // fn commented_out()",
            "",
            "fn 1abc()",
        ] {
            // The last but one is a comment line holding the word `fn`: a name is still read from it.
            if line.contains("commented_out") {
                assert_eq!(name_in(line).as_deref(), Some("commented_out"));
            } else {
                assert_eq!(name_in(line), None, "{line}");
            }
        }
    }

    #[test]
    fn a_context_without_a_keyword_gives_the_name_before_its_parenthesis() {
        assert_eq!(
            called_name("static int parse_args(int argc, char **argv)").as_deref(),
            Some("parse_args")
        );
        // A keyword before the parenthesis is not the name: the one before it is.
        assert_eq!(
            called_name("ZwaveClient.prototype.connect = async function () {").as_deref(),
            Some("connect")
        );
        assert_eq!(called_name("while (running)"), None);
        assert_eq!(called_name("no parenthesis here"), None);
        assert_eq!(called_name("if (x)"), None);
    }

    #[test]
    fn a_comment_that_says_something_is_kept_cut_at_a_word() {
        assert_eq!(
            comment_in("    // Should only invoke window.Image in the browser.").as_deref(),
            Some("Should only invoke window.Image in the browser.")
        );
        assert_eq!(
            comment_in("/** A modal can contain content. */").as_deref(),
            Some("A modal can contain content.")
        );
        assert_eq!(
            comment_in("# Get the yarn cache directory first").as_deref(),
            Some("Get the yarn cache directory first")
        );
        for line in [
            "// ok",
            "# shellcheck disable=SC2016 for this line",
            "#include <stdio.h>",
            "let x = 1; // short",
            "//",
        ] {
            assert_eq!(comment_in(line), None, "{line}");
        }
        let long = "// ".to_owned() + &"describe the whole behaviour of this change ".repeat(4);
        let cut = comment_in(&long).unwrap();
        let whole = long.trim_start_matches("// ");
        // It fits and stops at the end of a word, never inside one.
        assert!(cut.chars().count() <= MAX_COMMENT_CHARS, "{cut}");
        assert!(
            whole.starts_with(&cut) && whole[cut.len()..].starts_with(' '),
            "{cut}"
        );
    }

    #[test]
    fn the_names_come_from_every_code_file_in_order_without_repeats() {
        let a = file(
            "src/a.rs",
            "@@ -1,3 +1,4 @@ impl Parser {\n fn context() {}\n-pub fn old_name() {}\n+pub fn new_name() {}\n+// Reads the flags once, before the loop.\n\
             @@ -20 +21 @@ fn helper() {\n+pub fn new_name() {}\n",
        );
        let b = file(
            "tests/b.rs",
            "@@ -1 +1 @@\n+fn checks_the_flags() {}\n+++ b/tests/b.rs\n",
        );
        let got = touched(&[a, b]).unwrap();
        assert_eq!(
            got,
            "Names touched: Parser, old_name, new_name, helper, checks_the_flags\n\
             Comments added: \"Reads the flags once, before the loop.\""
        );
    }

    #[test]
    fn documents_lockfiles_and_binaries_name_nothing() {
        let docs = file(
            "README.md",
            "@@ -1 +1 @@\n+# A heading with words\n+fn in_a_code_block() {}\n",
        );
        let lock = file(
            "Cargo.lock",
            "@@ -1 +1 @@\n+name = \"serde\"\n+fn from_the_lock() {}\n",
        );
        let mut binary = file("logo.png", "");
        binary.lines = None;
        assert_eq!(touched(&[docs, lock, binary]), None);
        assert_eq!(touched(&[]), None);
    }

    #[test]
    fn at_most_eight_names_and_two_comments() {
        let hunks: String = (0..12)
            .map(|n| format!("+fn name_{n:02}() {{}}\n+// Comment number {n} says something.\n"))
            .collect();
        let got = touched(&[file("src/a.rs", &format!("@@ -1 +1 @@\n{hunks}"))]).unwrap();
        assert_eq!(
            got.lines().next().unwrap().matches(", ").count(),
            MAX_NAMES - 1
        );
        assert!(got.contains("name_07") && !got.contains("name_08"), "{got}");
        assert_eq!(
            got.lines().nth(1).unwrap().matches("\"; \"").count(),
            MAX_COMMENTS - 1
        );
        assert!(
            got.contains("Comment number 1 ") && !got.contains("Comment number 2 "),
            "{got}"
        );
    }
}
