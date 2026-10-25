//! `sc-tools release-notes TAG [REV]`: the notes of a release, from the
//! commits since the previous `v*` tag, grouped by their Conventional
//! Commits type, plus how to install. The release workflow publishes them.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail};

/// The `v*` tag before `rev`, if any.
fn previous_tag(repo: &Path, rev: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["describe", "--tags", "--match", "v*", "--abbrev=0"])
        .arg(format!("{rev}^"))
        .output()
        .ok()?;
    let tag = String::from_utf8(output.stdout).ok()?;
    (output.status.success() && !tag.trim().is_empty()).then(|| tag.trim().to_owned())
}

/// The subjects of the commits in `range`, merges excluded.
fn subjects(repo: &Path, range: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["log", "--no-merges", "--format=%s", range])
        .output()
        .context("cannot run git")?;
    if !output.status.success() {
        bail!(
            "git log {range} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect())
}

/// `type(scope)!: description` as (type, scope, description), when it is.
fn conventional(subject: &str) -> Option<(&str, &str, &str)> {
    let (head, description) = subject.split_once(": ")?;
    let head = head.strip_suffix('!').unwrap_or(head);
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => (kind, rest.strip_suffix(')').filter(|s| !s.contains(')'))?),
        None => (head, ""),
    };
    (!kind.is_empty() && kind.bytes().all(|b| b.is_ascii_lowercase())).then_some((
        kind,
        scope,
        description,
    ))
}

/// A list line: the scope in bold before the description, if it has one.
fn line(scope: &str, description: &str) -> String {
    if scope.is_empty() {
        format!("- {description}\n")
    } else {
        format!("- **{scope}** {description}\n")
    }
}

const SECTIONS: [(&str, &str); 4] = [
    ("Features", "feat"),
    ("Fixes", "fix"),
    ("Performance", "perf"),
    ("Documentation", "docs"),
];

/// The notes of `tag`, given the previous tag and the subjects since it.
pub fn render(tag: &str, previous: Option<&str>, subjects: &[String]) -> String {
    let mut text = format!(
        "## smart-commit {}\n\n",
        tag.strip_prefix('v').unwrap_or(tag)
    );
    match previous {
        Some(previous) => text.push_str(&format!("Changes since {previous}.\n\n")),
        None => text.push_str("First release.\n\n"),
    }
    for (title, kind) in SECTIONS {
        let lines: Vec<String> = subjects
            .iter()
            .filter_map(|s| conventional(s))
            .filter(|(k, _, _)| *k == kind)
            .map(|(_, scope, description)| line(scope, description))
            .collect();
        if !lines.is_empty() {
            text.push_str(&format!("### {title}\n\n{}\n", lines.concat()));
        }
    }
    let others: Vec<String> = subjects
        .iter()
        .filter(|s| !matches!(conventional(s), Some((k, _, _)) if SECTIONS.iter().any(|(_, t)| *t == k)))
        .map(|s| match conventional(s) {
            Some((_, scope, description)) => line(scope, description),
            None => format!("- {s}\n"),
        })
        .collect();
    if !others.is_empty() {
        text.push_str(&format!("### Other\n\n{}\n", others.concat()));
    }
    text.push_str(
        "### Install\n\n```bash\n\
         curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash\n\
         ```\n\n\
         The tarball is reproducible: `scripts/package.sh` on this tag gives the bytes\n\
         in `SHA256SUMS`, on any machine with Docker.\n",
    );
    text
}

/// The notes of `tag`, from the repository at `repo`, up to `rev`.
pub fn notes(repo: &Path, tag: &str, rev: &str) -> Result<String> {
    let previous = previous_tag(repo, rev);
    let range = match &previous {
        Some(previous) => format!("{previous}..{rev}"),
        None => rev.to_owned(),
    };
    Ok(render(tag, previous.as_deref(), &subjects(repo, &range)?))
}

#[cfg(test)]
#[path = "release_notes_tests.rs"]
mod tests;
