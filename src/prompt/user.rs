//! User message construction (files + diff + optional note).

use std::fmt::Write as _;

use crate::diff::CompactDiff;

pub(super) fn user_message(diff: &CompactDiff, note: Option<&str>) -> String {
    let mut text = format!("Files changed:\n{}", diff.stat);
    if !diff.patch.is_empty() {
        let _ = write!(text, "\nDiff:\n{}", diff.patch);
    }
    if diff.truncated > 0 {
        let _ = write!(
            text,
            "\n({} files are only partly shown; rely on the file list.)\n",
            diff.truncated
        );
    }
    if let Some(note) = note {
        let _ = write!(text, "\n{note}\n");
    }
    text
}
