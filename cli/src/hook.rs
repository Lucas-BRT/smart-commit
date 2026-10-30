//! The `prepare-commit-msg` hook: what gets installed, and how it fills in
//! the message file git prepared.
//!
//! The hook only acts on a plain `git commit` that is about to open the
//! editor. With `-m`/`-F`/`-t`, merges, squashes and amends, git already
//! has a message and the hook leaves it alone.

use std::path::Path;

/// Identifies hooks we installed, so we never overwrite or remove others.
pub const MARKER: &str = "# smart-commit prepare-commit-msg hook";

/// The hook script, calling `binary` by absolute path: GUI git clients
/// often run hooks without the user's shell `PATH`.
pub fn script(binary: &Path) -> String {
    let binary = shell_quote(&binary.to_string_lossy());
    // Without the binary (uninstalled, moved) the hook must not stop the
    // commit: `exec` of a missing file exits 127, and git aborts on that.
    format!(
        "#!/bin/sh\n\
         {MARKER}\n\
         # Fills in the commit message; `smart-commit hook uninstall` removes it.\n\
         [ -x {binary} ] || exit 0\n\
         exec {binary} hook run \"$@\"\n"
    )
}

pub fn is_ours(script: &str) -> bool {
    script.lines().any(|line| line == MARKER)
}

/// Whether to write a message, given the hook's second argument. It is
/// absent for a plain `git commit`, and names where the message came from
/// otherwise: `message`, `template`, `merge`, `squash` or `commit`.
pub fn should_fill(source: Option<&str>) -> bool {
    source.is_none_or(str::is_empty)
}

/// Puts `message` before the comments git wrote in the message file.
///
/// Returns `None` if the file already has a message, so nothing written by
/// the user or another tool is lost. With `git commit -v`, everything after
/// the scissors line is the diff and does not count.
pub fn prefill(existing: &str, message: &str, comment: &str) -> Option<String> {
    let scissors = format!("{comment} ------------------------ >8 ------------------------");
    let has_message = existing
        .lines()
        .take_while(|line| *line != scissors)
        .any(|line| !line.trim().is_empty() && !line.starts_with(comment));
    if has_message {
        return None;
    }
    Some(format!("{message}\n{existing}"))
}

/// Single-quotes `text` for `sh`.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = "\n\
        # Please enter the commit message for your changes. Lines starting\n\
        # with '#' will be ignored, and an empty message aborts the commit.\n\
        #\n\
        # On branch main\n";

    #[test]
    fn script_calls_the_binary_by_absolute_path() {
        let script = script(Path::new("/opt/npu commit/bin/smart-commit"));
        assert_eq!(
            script,
            "#!/bin/sh\n\
             # smart-commit prepare-commit-msg hook\n\
             # Fills in the commit message; `smart-commit hook uninstall` removes it.\n\
             [ -x '/opt/npu commit/bin/smart-commit' ] || exit 0\n\
             exec '/opt/npu commit/bin/smart-commit' hook run \"$@\"\n"
        );
        assert!(is_ours(&script));
    }

    #[test]
    fn quotes_single_quotes_in_the_path() {
        assert_eq!(shell_quote("/it's/here"), r"'/it'\''s/here'");
    }

    #[test]
    fn other_hooks_are_not_ours() {
        assert!(!is_ours("#!/bin/sh\nexec lint-commit \"$@\"\n"));
        assert!(!is_ours(&format!("#!/bin/sh\necho '{MARKER}'\n")));
    }

    #[test]
    fn fills_only_plain_commits() {
        assert!(should_fill(None));
        assert!(should_fill(Some("")));
        for source in ["message", "template", "merge", "squash", "commit"] {
            assert!(!should_fill(Some(source)), "{source}");
        }
    }

    #[test]
    fn message_goes_before_the_comments() {
        assert_eq!(
            prefill(TEMPLATE, "feat: add the hook\n\nBody.", "#").unwrap(),
            format!("feat: add the hook\n\nBody.\n{TEMPLATE}")
        );
        assert_eq!(prefill("", "feat: x", "#").unwrap(), "feat: x\n");
    }

    #[test]
    fn existing_message_is_kept() {
        let with_message = format!("WIP: my own words\n{TEMPLATE}");
        assert_eq!(prefill(&with_message, "feat: x", "#"), None);
    }

    #[test]
    fn verbose_diff_after_the_scissors_does_not_count() {
        let verbose = format!(
            "{TEMPLATE}# ------------------------ >8 ------------------------\n\
             # Do not modify or remove the line above.\n\
             diff --git a/a.rs b/a.rs\n+new line\n"
        );
        assert!(prefill(&verbose, "feat: x", "#").is_some());
    }

    #[test]
    fn custom_comment_char() {
        let template = "\n; Please enter the commit message\n";
        assert!(prefill(template, "feat: x", ";").is_some());
        assert_eq!(prefill(template, "feat: x", "#"), None);
    }
}
