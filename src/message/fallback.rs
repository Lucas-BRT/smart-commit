//! Plain messages built from the file list when the model fails.

use crate::diff::FileKind;
use crate::git::{ChangeStatus, FileChange};
use crate::style::{self, Convention, Language};

use super::CommitMessage;

/// Files listed in a fallback body before summarising the rest.
const FALLBACK_LISTED: usize = 10;

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
    fallback_after(changes, convention, language, None, super::SUBJECT_MAX)
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
        subject: super::format::truncate_subject(&subject, subject_max),
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
pub(crate) fn common_dir(changes: &[FileChange]) -> Option<String> {
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
