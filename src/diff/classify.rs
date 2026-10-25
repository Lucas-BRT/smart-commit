use super::{FileDiff, FileKind};

const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
    "flake.lock",
    "mix.lock",
    "Podfile.lock",
    "pubspec.lock",
    "packages.lock.json",
];
const GENERATED_DIRS: &[&str] = &[
    "dist",
    "build",
    "target",
    "vendor",
    "node_modules",
    "__pycache__",
    "generated",
];
const GENERATED_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.css",
    ".map",
    ".pb.go",
    "_pb2.py",
    ".g.dart",
    ".freezed.dart",
];
const TEST_DIRS: &[&str] = &["test", "tests", "__tests__", "spec", "specs", "testdata"];
const DOC_DIRS: &[&str] = &["doc", "docs"];
const DOC_EXTENSIONS: &[&str] = &["md", "mdx", "rst", "adoc", "txt"];
const DOC_STEMS: &[&str] = &["readme", "changelog", "license", "contributing", "authors"];
const CONFIG_EXTENSIONS: &[&str] = &["toml", "yaml", "yml", "json", "ini", "cfg", "conf", "env"];

/// Markers code generators put at the top of what they write: Cargo's and
/// Facebook's `@generated`, Go's `Code generated ... DO NOT EDIT.`, protoc,
/// sqlc, OpenAPI and many more.
const GENERATED_MARKERS: &[&str] = &["@generated", "DO NOT EDIT"];
/// How far from the top of a file a marker counts.
const MARKER_LINES: usize = 5;

/// Whether a tool rather than a person wrote `file`, on strong evidence
/// only: a known lockfile name, a generator's marker in its first lines, or
/// `linguist-generated` in `.gitattributes` (`generated` lists those paths).
/// Unlike the guesses of [`FileKind`] from directories such as `build/`,
/// this is sure enough to write a message without asking the model. An
/// unknown lockfile just goes to the model, as any file does.
pub fn machine_written(file: &FileDiff, generated: &[String]) -> bool {
    FileKind::of_path(&file.path) == FileKind::Lockfile
        || generated.contains(&file.path)
        || has_generated_marker(&file.hunks)
}

/// Looks for a marker in the first [`MARKER_LINES`] lines of the new file,
/// which the patch shows only when its first hunk starts there.
fn has_generated_marker(hunks: &str) -> bool {
    let mut lines = hunks.lines();
    let starts_at_top = lines.next().is_some_and(|header| {
        header
            .split_whitespace()
            .nth(2)
            .and_then(|new| new.strip_prefix('+'))
            .and_then(|range| range.split(',').next())
            .is_some_and(|start| start == "1" || start == "0")
    });
    starts_at_top
        && lines
            .filter(|line| !line.starts_with('-') && !line.starts_with('\\'))
            .take(MARKER_LINES)
            .any(|line| GENERATED_MARKERS.iter().any(|marker| line.contains(marker)))
}

pub(super) fn classify_path(path: &str) -> FileKind {
    let mut components: Vec<&str> = path.split('/').collect();
    let name = components.pop().unwrap_or(path);
    let lower = name.to_lowercase();
    let (stem, extension) = lower.rsplit_once('.').unwrap_or((&lower, ""));
    let in_dir = |dirs: &[&str]| components.iter().any(|c| dirs.contains(c));

    if LOCKFILES.contains(&name) || extension == "lock" {
        FileKind::Lockfile
    } else if in_dir(GENERATED_DIRS)
        || GENERATED_SUFFIXES.iter().any(|s| lower.ends_with(s))
        || lower.contains(".generated.")
    {
        FileKind::Generated
    } else if in_dir(TEST_DIRS)
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
    {
        FileKind::Test
    } else if in_dir(DOC_DIRS) || DOC_EXTENSIONS.contains(&extension) || DOC_STEMS.contains(&stem) {
        FileKind::Docs
    } else if CONFIG_EXTENSIONS.contains(&extension)
        || name.starts_with('.')
        || matches!(lower.as_str(), "dockerfile" | "makefile")
    {
        FileKind::Config
    } else {
        FileKind::Source
    }
}
