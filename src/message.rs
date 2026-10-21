//! Cleans up what the model writes, and the message used when it fails.
//!
//! Small models wrap the answer in code fences, quote it, announce it
//! ("Here is the commit message:"), explain it afterwards, emit Qwen3
//! `<think>` blocks or overflow 72 columns. All of that is fixed here,
//! deterministically, so the backend only has to get the content right.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::diff::FileKind;
use crate::git::{ChangeStatus, FileChange};
use crate::style::{self, Convention, Language};

pub const SUBJECT_MAX: usize = 72;
pub const BODY_WIDTH: usize = 72;
/// Files listed in a fallback body before summarising the rest.
const FALLBACK_LISTED: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMessage {
    pub subject: String,
    pub body: Option<String>,
}

impl fmt::Display for CommitMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.subject)?;
        if let Some(body) = &self.body {
            write!(f, "\n\n{body}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SanitizeError {
    #[error("the model returned no message")]
    Empty,
    #[error("subject does not follow Conventional Commits: {0:?}")]
    NotConventional(String),
    /// The subject repeats one from the history, as small models do when
    /// the history is short or the diff says little.
    #[error("subject repeats an earlier commit: {0:?}")]
    CopiedHistory(String),
    /// The subject is clearly in another language than the repository's,
    /// as small models trained on English write in Portuguese ones.
    #[error("subject is in another language than the repository's: {0:?}")]
    WrongLanguage(String),
    /// The subject repeats, or nearly, a message the user asked to replace:
    /// with greedy decoding the note asking for another is not always
    /// enough.
    #[error("subject repeats a message the user turned down: {0:?}")]
    TurnedDown(String),
}

/// Labels the model puts before the message, compared lowercase.
const LABELS: &[&str] = &[
    "suggested commit message",
    "commit message",
    "mensagem de commit sugerida",
    "mensagem de commit",
    "mensagem do commit",
    "mensagem",
    "subject",
    "assunto",
    "título",
    "titulo",
];
/// Openers of a whole preamble line such as "Here is the commit message:".
const PREAMBLES: &[&str] = &[
    "here is",
    "here's",
    "sure",
    "segue",
    "aqui está",
    "aqui esta",
];
/// Openers of an explanation after the message; everything from there on
/// is dropped.
const EXPLANATIONS: &[&str] = &[
    "explanation",
    "this commit message",
    "this message",
    "the commit message",
    "note:",
    "explicação",
    "explicacao",
    "esta mensagem",
    "essa mensagem",
    "nota:",
    "obs:",
];

pub fn sanitize(raw: &str, convention: Convention) -> Result<CommitMessage, SanitizeError> {
    sanitize_within(raw, convention, SUBJECT_MAX)
}

/// Like [`sanitize`], fitting the subject in `subject_max` columns.
pub fn sanitize_within(
    raw: &str,
    convention: Convention,
    subject_max: usize,
) -> Result<CommitMessage, SanitizeError> {
    let text = strip_thinking(raw);
    let text = unfence(&text);

    let mut lines = text
        .lines()
        .map(str::trim_end)
        .skip_while(|l| l.trim().is_empty() || is_preamble(l));
    let subject = lines.next().map(clean_subject).unwrap_or_default();
    if !subject.chars().any(char::is_alphanumeric) {
        return Err(SanitizeError::Empty);
    }
    let subject = match convention {
        Convention::Conventional => fix_conventional(&subject)?,
        _ => subject,
    };

    let body_lines: Vec<&str> = lines.take_while(|l| !is_explanation(l)).collect();
    let body = wrap_body(&body_lines);
    Ok(CommitMessage {
        subject: truncate_subject(&subject, subject_max),
        body: (!body.is_empty()).then_some(body),
    })
}

/// Drops `<think>…</think>`; an unclosed block swallows the rest.
fn strip_thinking(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The contents of the first code fence, or the text if there is none.
fn unfence(text: &str) -> String {
    let is_fence = |l: &str| l.trim_start().starts_with("```");
    if !text.lines().any(is_fence) {
        return text.to_owned();
    }
    let inside: Vec<&str> = text
        .lines()
        .skip_while(|l| !is_fence(l))
        .skip(1)
        .take_while(|l| !is_fence(l))
        .collect();
    if inside.iter().any(|l| !l.trim().is_empty()) {
        inside.join("\n")
    } else {
        // An empty fence: the message was probably written around it.
        text.lines()
            .filter(|l| !is_fence(l))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn is_preamble(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    let Some(head) = lower.strip_suffix(':') else {
        return false;
    };
    PREAMBLES.iter().any(|p| head.starts_with(p)) || LABELS.contains(&head.trim_matches('*'))
}

fn is_explanation(line: &str) -> bool {
    let lower = line.trim().trim_start_matches(['*', '_']).to_lowercase();
    EXPLANATIONS.iter().any(|e| lower.starts_with(e))
}

fn clean_subject(line: &str) -> String {
    let mut s = line.trim();
    s = s.trim_start_matches('#').trim_start();
    s = strip_list_marker(s);
    s = strip_label(s);
    s = unwrap_pairs(s);
    let mut subject = s.split_whitespace().collect::<Vec<_>>().join(" ");
    while subject.ends_with('.') && !subject.ends_with("..") {
        subject.pop();
    }
    subject
}

/// `- x`, `* x`, `1. x`, `2) x`.
fn strip_list_marker(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")) {
        return rest.trim_start();
    }
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = s[digits..]
            .strip_prefix(". ")
            .or_else(|| s[digits..].strip_prefix(") "))
    {
        return rest.trim_start();
    }
    s
}

/// `Commit message: fix: x` → `fix: x`, also when the label is bold.
fn strip_label(s: &str) -> &str {
    let Some((head, rest)) = s.split_once(':') else {
        return s;
    };
    let label = head.trim().trim_matches('*').trim().to_lowercase();
    if LABELS.contains(&label.as_str()) {
        rest.trim_start_matches('*').trim()
    } else {
        s
    }
}

/// Repeatedly removes matching quotes, backticks or bold markers around
/// the whole subject.
fn unwrap_pairs(mut s: &str) -> &str {
    const PAIRS: &[(&str, &str)] = &[
        ("**", "**"),
        ("\"", "\""),
        ("'", "'"),
        ("`", "`"),
        ("“", "”"),
        ("‘", "’"),
    ];
    loop {
        let unwrapped = PAIRS.iter().find_map(|(open, close)| {
            s.strip_prefix(open)
                .and_then(|r| r.strip_suffix(close))
                .filter(|inner| !inner.trim().is_empty())
        });
        match unwrapped {
            Some(inner) => s = inner.trim(),
            None => return s,
        }
    }
}

/// Accepts the common near misses: `Feat: x`, `feat:x`, `feat : x`.
fn fix_conventional(subject: &str) -> Result<String, SanitizeError> {
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

/// Fits a subject in `max` columns, without an ellipsis (git
/// tooling shows the subject as is): first a scope that is a file path
/// becomes the file's name (`fix(src/operator/combineLatest.ts)` →
/// `fix(combineLatest)`), then the subject ends at the last boundary of
/// [`BOUNDARIES`] that keeps at least half of the limit, else at the last
/// word that fits.
fn truncate_subject(subject: &str, max: usize) -> String {
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

/// What a cut subject should not end with.
const TRAILING: [char; 5] = [' ', ',', ';', ':', '-'];

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

/// Re-wraps paragraphs and `- ` bullets at [`BODY_WIDTH`].
fn wrap_body(lines: &[&str]) -> String {
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
            if !empty && line.chars().count() + 1 + word.chars().count() > BODY_WIDTH {
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

/// What a set of changed files is about, for the fallback's type or emoji.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Docs,
    Tests,
    Config,
    Mixed,
}

impl Category {
    fn of(changes: &[FileChange]) -> Self {
        let kinds: Vec<FileKind> = changes.iter().map(|c| FileKind::of_path(&c.path)).collect();
        let all = |pred: fn(&FileKind) -> bool| !kinds.is_empty() && kinds.iter().all(pred);
        if all(|k| *k == FileKind::Docs) {
            Self::Docs
        } else if all(|k| *k == FileKind::Test) {
            Self::Tests
        } else if all(|k| matches!(k, FileKind::Config | FileKind::Lockfile)) {
            Self::Config
        } else {
            Self::Mixed
        }
    }

    fn conventional_type(self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::Tests => "test",
            Self::Config | Self::Mixed => "chore",
        }
    }

    fn gitmoji(self) -> &'static str {
        match self {
            Self::Docs => "📝",
            Self::Tests => "✅",
            Self::Config => "🔧",
            Self::Mixed => "🚧",
        }
    }
}

/// A plain message built from the file list alone, for when the model's
/// output is still unusable after a retry.
pub fn fallback(
    changes: &[FileChange],
    convention: Convention,
    language: Language,
) -> CommitMessage {
    fallback_after(changes, convention, language, None, SUBJECT_MAX)
}

/// Like [`fallback`], keeping the scope of `reply`, the model's last
/// subject, when it was rejected for what it said (a copy of the history,
/// another language) rather than its form, and its type when the files do
/// not tell one: the model often picks the right type even when it copies,
/// and the file list's guess for code (`chore`) is worse. Files that are
/// all docs, tests or config keep their type (`docs`, `test`, `chore`).
pub fn fallback_after(
    changes: &[FileChange],
    convention: Convention,
    language: Language,
    reply: Option<&str>,
    subject_max: usize,
) -> CommitMessage {
    let pt = language == Language::Portuguese;
    let description = describe_changes(changes, pt);
    let category = Category::of(changes);
    let borrowed = reply.and_then(style::parse_conventional);
    let subject = match (convention, borrowed) {
        (Convention::Conventional | Convention::Unknown, Some(header)) => {
            let kind = match category {
                Category::Mixed => header.kind,
                known => known.conventional_type(),
            };
            match header.scope {
                Some(scope) => format!("{kind}({scope}): {description}"),
                None => format!("{kind}: {description}"),
            }
        }
        (Convention::Conventional | Convention::Unknown, None) => {
            format!("{}: {description}", category.conventional_type())
        }
        (Convention::Gitmoji, _) => format!("{} {description}", category.gitmoji()),
        (Convention::Free, _) => capitalize(&description),
    };
    CommitMessage {
        subject: truncate_subject(&subject, subject_max),
        body: (changes.len() > 1).then(|| list_files(changes, pt)),
    }
}

fn describe_changes(changes: &[FileChange], pt: bool) -> String {
    let [one] = changes else {
        let (verb, files, within) = if pt {
            ("atualiza", "arquivos", "em")
        } else {
            ("update", "files", "in")
        };
        return match common_dir(changes) {
            Some(dir) => format!("{verb} {} {files} {within} {dir}", changes.len()),
            None => format!("{verb} {} {files}", changes.len()),
        };
    };
    let verb = match (one.status, pt) {
        (ChangeStatus::Added, false) => "add",
        (ChangeStatus::Added, true) => "adiciona",
        (ChangeStatus::Deleted, _) => "remove",
        (ChangeStatus::Renamed, false) => "rename",
        (ChangeStatus::Renamed, true) => "renomeia",
        (_, false) => "update",
        (_, true) => "atualiza",
    };
    match (&one.old_path, one.status) {
        (Some(old), ChangeStatus::Renamed) => {
            let to = if pt { "para" } else { "to" };
            format!("{verb} {old} {to} {}", one.path)
        }
        _ => format!("{verb} {}", one.path),
    }
}

fn list_files(changes: &[FileChange], pt: bool) -> String {
    let mut lines: Vec<String> = changes
        .iter()
        .take(FALLBACK_LISTED)
        .map(|c| format!("- {}", c.path))
        .collect();
    if changes.len() > FALLBACK_LISTED {
        let more = changes.len() - FALLBACK_LISTED;
        lines.push(if pt {
            format!("- ... e mais {more}")
        } else {
            format!("- ... and {more} more")
        });
    }
    lines.join("\n")
}

/// The deepest directory containing every changed path, if not the root.
fn common_dir(changes: &[FileChange]) -> Option<String> {
    let mut dirs = changes.iter().map(|c| {
        let mut parts: Vec<&str> = c.path.split('/').collect();
        parts.pop();
        parts
    });
    let mut common = dirs.next()?;
    for dir in dirs {
        let shared = common.iter().zip(&dir).take_while(|(a, b)| a == b).count();
        common.truncate(shared);
    }
    (!common.is_empty()).then(|| format!("{}/", common.join("/")))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(raw: &str) -> String {
        sanitize(raw, Convention::Free).unwrap().to_string()
    }

    fn conventional(raw: &str) -> Result<String, SanitizeError> {
        sanitize(raw, Convention::Conventional).map(|m| m.to_string())
    }

    #[test]
    fn plain_message_is_unchanged() {
        assert_eq!(clean("Add login page"), "Add login page");
        assert_eq!(
            clean("Add login page\n\nUsers can now sign in."),
            "Add login page\n\nUsers can now sign in."
        );
    }

    #[test]
    fn strips_fences_quotes_and_preambles() {
        for raw in [
            "```\nAdd login page\n```",
            "```text\nAdd login page\n```\n",
            "\"Add login page\"",
            "`Add login page`",
            "“Add login page”",
            "**Add login page**",
            "Here is the commit message:\n\nAdd login page",
            "Sure! Here's a commit message:\n```\nAdd login page\n```",
            "Commit message: Add login page",
            "**Commit message:** Add login page",
            "Mensagem de commit:\nAdd login page",
            "# Add login page",
            "- Add login page",
            "1. Add login page",
            "Add login page.",
            "  Add   login page  ",
        ] {
            assert_eq!(clean(raw), "Add login page", "{raw:?}");
        }
    }

    #[test]
    fn drops_thinking_blocks() {
        assert_eq!(
            clean("<think>\nThe diff adds a page.\n</think>\n\nAdd login page"),
            "Add login page"
        );
        assert_eq!(
            sanitize("<think>still thinking", Convention::Free),
            Err(SanitizeError::Empty)
        );
    }

    #[test]
    fn drops_every_thinking_block() {
        assert_eq!(
            clean("<think>a</think>Add login<think>b</think> page"),
            "Add login page"
        );
    }

    #[test]
    fn empty_fence_keeps_the_text_around_it() {
        assert_eq!(clean("Add login page\n```\n```"), "Add login page");
    }

    #[test]
    fn only_the_first_fence_is_used() {
        assert_eq!(
            clean("```\nAdd login page\n```\n\nor:\n```\nCreate login page\n```"),
            "Add login page"
        );
    }

    #[test]
    fn drops_explanations_after_the_message() {
        assert_eq!(
            clean("Add login page\n\nThis commit message describes the new page."),
            "Add login page"
        );
        assert_eq!(
            clean("Adiciona login\n\nPermite entrar.\n\n**Explicação:** segui o padrão."),
            "Adiciona login\n\nPermite entrar."
        );
    }

    #[test]
    fn empty_output_is_an_error() {
        for raw in ["", "   \n\n", "```\n```", "\"\"", "..."] {
            assert_eq!(
                sanitize(raw, Convention::Free),
                Err(SanitizeError::Empty),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn fixes_near_conventional_subjects() {
        assert_eq!(conventional("Feat: add login").unwrap(), "feat: add login");
        assert_eq!(conventional("feat:add login").unwrap(), "feat: add login");
        assert_eq!(
            conventional("Fix(api)!: drop v1").unwrap(),
            "fix(api)!: drop v1"
        );
        assert_eq!(
            conventional("```\nfeat(cli): add flag\n```").unwrap(),
            "feat(cli): add flag"
        );
    }

    #[test]
    fn rejects_non_conventional_subjects() {
        for raw in ["Add login page", "Here is: the message", "feat(: x"] {
            assert!(
                matches!(conventional(raw), Err(SanitizeError::NotConventional(_))),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn long_subject_is_cut_at_a_word() {
        let raw =
            "feat(parser): support renames, copies and type changes in the staged diff output";
        let subject = conventional(raw).unwrap();
        // At the phrase "in ...", not mid-phrase ("... in the staged").
        assert_eq!(
            subject,
            "feat(parser): support renames, copies and type changes"
        );
        assert!(subject.chars().count() <= SUBJECT_MAX);

        let unbroken = "x".repeat(100);
        assert_eq!(clean(&unbroken).chars().count(), SUBJECT_MAX);
        let accented = format!("Atualiza {}", "ç".repeat(100));
        assert_eq!(clean(&accented), "Atualiza");
    }

    #[test]
    fn the_limit_is_the_configured_one() {
        let raw =
            "fix(kubernetes): update manifest and pod handlers to handle job and pod conditions";
        // 82 columns fit in 100; in 60 the subject ends at "to handle".
        let wide = sanitize_within(raw, Convention::Conventional, 100).unwrap();
        assert_eq!(wide.subject, raw);
        let narrow = sanitize_within(raw, Convention::Conventional, 60).unwrap();
        assert_eq!(
            narrow.subject,
            "fix(kubernetes): update manifest and pod handlers"
        );
        let changes = [change(
            ChangeStatus::Modified,
            "src/a/very/long/path/to/some/module/file.rs",
        )];
        let fallback = fallback_after(
            &changes,
            Convention::Conventional,
            Language::English,
            None,
            40,
        );
        assert!(
            fallback.subject.chars().count() <= 40,
            "{}",
            fallback.subject
        );
    }

    #[test]
    fn long_subjects_end_where_they_still_read() {
        // Replies of qwen2.5-coder-1.5b on eval-v1, over the limit.
        for (raw, subject) in [
            (
                "fix(kubernetes): update manifest and pod handlers to handle job and pod conditions",
                "fix(kubernetes): update manifest and pod handlers",
            ),
            (
                "fix(clouddriver-core): update RetrofitConfig to support capacity provider strategies",
                "fix(clouddriver-core): update RetrofitConfig",
            ),
            (
                "fix(src/operator/combineLatest.ts): update slice method to avoid unnecessary copying",
                "fix(combineLatest): update slice method to avoid unnecessary copying",
            ),
            (
                "fix(doc/lettable-operators.md): update doc on building with the rxjs/operators import location",
                "fix(lettable-operators): update doc on building",
            ),
            (
                "fix(provider/kubernetes): Update cluster provider logic to handle missing providers",
                "fix(provider/kubernetes): Update cluster provider logic",
            ),
            (
                "refactor(src/internal/Subscriber.ts, src/internal/scheduler/AnimationFrameAction.ts): use the new names",
                "refactor(Subscriber): use the new names",
            ),
        ] {
            assert_eq!(conventional(raw).unwrap(), subject, "{raw}");
        }
        // A path scope is kept when the subject fits: only length shortens it.
        assert_eq!(
            conventional("fix(src/lib.rs): handle empty input").unwrap(),
            "fix(src/lib.rs): handle empty input"
        );
        // A boundary that would keep too little falls back to the last word.
        let raw = "fix: crash, the parser reads the whole staged diff output twice over every time";
        let subject = conventional(raw).unwrap();
        assert!(
            subject.starts_with("fix: crash, the parser reads"),
            "{subject}"
        );
        assert!(subject.chars().count() <= SUBJECT_MAX);
    }

    #[test]
    fn body_is_rewrapped() {
        let raw = "Add login page\n\nThis lets users sign in with their email address and a password, \
                   which was the most requested feature.\n\n* first point\n* second point that is long \
                   enough to need wrapping onto a second line of the body";
        insta::assert_snapshot!(clean(raw), @r"
        Add login page

        This lets users sign in with their email address and a password, which
        was the most requested feature.

        - first point
        - second point that is long enough to need wrapping onto a second line
          of the body
        ");
    }

    fn change(status: ChangeStatus, path: &str) -> FileChange {
        FileChange {
            status,
            path: path.to_owned(),
            old_path: None,
        }
    }

    #[test]
    fn fallback_after_a_rejected_reply() {
        let code = [change(ChangeStatus::Modified, "src/git.rs")];
        let docs = [change(ChangeStatus::Modified, "AGENTS.md")];
        let after = |changes: &[FileChange], reply| {
            fallback_after(
                changes,
                Convention::Conventional,
                Language::English,
                reply,
                SUBJECT_MAX,
            )
            .subject
        };
        // Code: the reply's type and scope beat the file list's `chore`.
        assert_eq!(
            after(&code, Some("fix(git): handle detached HEAD")),
            "fix(git): update src/git.rs"
        );
        assert_eq!(after(&code, Some("feat: add x")), "feat: update src/git.rs");
        // Docs only: the files tell the type; the scope is still kept
        // (fixture conventional-en-partial-copy).
        assert_eq!(
            after(&docs, Some("fix(agents): trace a docs-only change")),
            "docs(agents): update AGENTS.md"
        );
        // No reply, or one without a header: as before.
        assert_eq!(after(&code, None), "chore: update src/git.rs");
        assert_eq!(
            after(&code, Some("Handle detached HEAD")),
            "chore: update src/git.rs"
        );
        // Free style has no type to keep.
        assert_eq!(
            fallback_after(
                &code,
                Convention::Free,
                Language::English,
                Some("fix(git): x"),
                SUBJECT_MAX,
            )
            .subject,
            "Update src/git.rs"
        );
    }

    #[test]
    fn fallback_for_a_single_file() {
        let added = [change(ChangeStatus::Added, "src/diff.rs")];
        assert_eq!(
            fallback(&added, Convention::Conventional, Language::English).to_string(),
            "chore: add src/diff.rs"
        );
        assert_eq!(
            fallback(&added, Convention::Free, Language::Portuguese).to_string(),
            "Adiciona src/diff.rs"
        );
        let renamed = [FileChange {
            old_path: Some("a.md".into()),
            ..change(ChangeStatus::Renamed, "docs/b.md")
        }];
        assert_eq!(
            fallback(&renamed, Convention::Gitmoji, Language::Portuguese).to_string(),
            "📝 renomeia a.md para docs/b.md"
        );
    }

    #[test]
    fn fallback_for_many_files_lists_them() {
        let changes: Vec<FileChange> = (0..12)
            .map(|n| change(ChangeStatus::Modified, &format!("tests/case_{n}.rs")))
            .collect();
        let message = fallback(&changes, Convention::Conventional, Language::Portuguese);
        assert_eq!(message.subject, "test: atualiza 12 arquivos em tests/");
        let body = message.body.unwrap();
        assert_eq!(body.lines().count(), 11);
        assert!(body.ends_with("- ... e mais 2"));
    }

    #[test]
    fn fallback_picks_type_and_emoji_from_the_files() {
        let config = [
            change(ChangeStatus::Modified, "Cargo.toml"),
            change(ChangeStatus::Modified, "Cargo.lock"),
        ];
        assert_eq!(
            fallback(&config, Convention::Gitmoji, Language::English).subject,
            "🔧 update 2 files"
        );
        assert_eq!(
            fallback(&config, Convention::Conventional, Language::English).subject,
            "chore: update 2 files"
        );
        let mixed = [
            change(ChangeStatus::Modified, "src/a.rs"),
            change(ChangeStatus::Deleted, "docs/b.md"),
        ];
        assert_eq!(
            fallback(&mixed, Convention::Gitmoji, Language::English).subject,
            "🚧 update 2 files"
        );
        assert_eq!(
            fallback(&mixed, Convention::Free, Language::English).to_string(),
            "Update 2 files\n\n- src/a.rs\n- docs/b.md"
        );
        let deleted = [change(ChangeStatus::Deleted, "docs/old.md")];
        assert_eq!(
            fallback(&deleted, Convention::Unknown, Language::Unknown).subject,
            "docs: remove docs/old.md"
        );
    }

    #[test]
    fn fallback_subject_fits_72_columns() {
        let path = format!("src/{}/mod.rs", "deeply_nested".repeat(8));
        let message = fallback(
            &[change(ChangeStatus::Added, &path)],
            Convention::Conventional,
            Language::English,
        );
        assert!(message.subject.chars().count() <= SUBJECT_MAX);
    }

    #[test]
    fn common_dir_is_the_shared_prefix() {
        let at = |paths: &[&str]| {
            common_dir(
                &paths
                    .iter()
                    .map(|p| change(ChangeStatus::Modified, p))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(at(&["src/a/x.rs", "src/a/y.rs"]).as_deref(), Some("src/a/"));
        assert_eq!(at(&["src/a/x.rs", "src/b.rs"]).as_deref(), Some("src/"));
        assert_eq!(at(&["src/x.rs", "README.md"]), None);
    }
}
