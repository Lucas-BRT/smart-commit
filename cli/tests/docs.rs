//! The Markdown pages describe the repository as it is: the paths they name
//! exist and every command is in the usage guide.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root, one level above this crate.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Directories that are not documentation: build output, ignored data and
/// the synthetic repositories of the training scenarios.
const SKIPPED: &[&str] = &[
    "target",
    ".git",
    // The website is documented by its own pages, for its own tree.
    "website",
    "training/data",
    "training/runs",
    "training/scenarios/repos",
    "training/.venv",
];

/// The first directory of a path that a page may name from the root.
const ROOT_DIRS: &[&str] = &["cli/", "scripts/", "docs/", ".github/"];

fn pages(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(ROOT).unwrap().to_string_lossy();
        if SKIPPED.iter().any(|s| relative == *s) {
            continue;
        }
        if path.is_dir() {
            pages(&path, found);
        } else if path.extension().is_some_and(|e| e == "md") {
            found.push(path);
        }
    }
}

fn all_pages() -> Vec<PathBuf> {
    let mut found = Vec::new();
    pages(Path::new(ROOT), &mut found);
    assert!(!found.is_empty());
    found
}

/// The inline `code` spans of a page, outside fenced blocks (which hold
/// examples, not references).
fn inline_code(text: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        spans.extend(line.split('`').skip(1).step_by(2).map(str::to_owned));
    }
    spans
}

#[test]
fn paths_named_in_the_pages_exist() {
    let mut missing = Vec::new();
    for page in all_pages() {
        let text = fs::read_to_string(&page).unwrap();
        for span in inline_code(&text) {
            let named = span.trim_end_matches('/');
            let checkable = ROOT_DIRS.iter().any(|d| named.starts_with(d))
                && !named.contains(['*', '<', '{', '$', ' ', '(', ':']);
            if checkable && !Path::new(ROOT).join(named).exists() {
                let page = page.strip_prefix(ROOT).unwrap().display();
                missing.push(format!("{page}: `{span}`"));
            }
        }
    }
    assert!(missing.is_empty(), "missing paths:\n{}", missing.join("\n"));
}

#[test]
fn every_command_is_in_the_usage_guide() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_smart-commit"));
    let output = common::isolate(command.arg("--help")).output().unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    let commands: Vec<&str> = help
        .lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|name| *name != "help")
        .collect();
    assert!(commands.len() > 10, "no commands found in:\n{help}");

    let usage = fs::read_to_string(Path::new(ROOT).join("docs/usage.md")).unwrap();
    let missing: Vec<&&str> = commands
        .iter()
        .filter(|name| !usage.contains(&format!("smart-commit {name}")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/usage.md does not mention: {missing:?}"
    );
}

/// GitHub's anchor for a heading: lowercase, punctuation dropped, spaces as
/// hyphens.
fn anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn anchors(text: &str) -> Vec<String> {
    let mut fenced = false;
    let mut found = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.starts_with('#') {
            found.push(anchor(line.trim_start_matches('#')));
        }
    }
    found
}

#[test]
fn links_between_the_pages_resolve() {
    let mut broken = Vec::new();
    for page in all_pages() {
        let text = fs::read_to_string(&page).unwrap();
        let name = page.strip_prefix(ROOT).unwrap().display().to_string();
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            if fenced {
                continue;
            }
            for target in line.split("](").skip(1) {
                let Some((target, _)) = target.split_once(')') else {
                    continue;
                };
                if target.starts_with("http") || target.starts_with("mailto:") {
                    continue;
                }
                let (file, fragment) = target.split_once('#').unwrap_or((target, ""));
                let path = if file.is_empty() {
                    page.clone()
                } else {
                    page.parent().unwrap().join(file)
                };
                if !path.exists() {
                    broken.push(format!("{name}: {target} (no such file)"));
                } else if !fragment.is_empty()
                    && path.extension().is_some_and(|e| e == "md")
                    && !anchors(&fs::read_to_string(&path).unwrap()).contains(&fragment.to_owned())
                {
                    broken.push(format!("{name}: {target} (no such heading)"));
                }
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}
