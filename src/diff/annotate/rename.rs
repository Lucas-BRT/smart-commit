use crate::diff::FileDiff;

/// A new file sharing this share of its lines with a deleted one is
/// probably that file renamed, below git's 50% rename threshold.
const LIKELY_RENAME: f64 = 0.4;

/// `dir/{old => new}/rest`, git's short form of a rename: the common
/// leading and trailing directories are written once.
pub fn rename_path(old: &str, new: &str) -> String {
    let old_parts: Vec<&str> = old.split('/').collect();
    let new_parts: Vec<&str> = new.split('/').collect();
    let mut head = 0;
    while head + 1 < old_parts.len().min(new_parts.len()) && old_parts[head] == new_parts[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < old_parts.len() - head - 1
        && tail < new_parts.len() - head - 1
        && old_parts[old_parts.len() - 1 - tail] == new_parts[new_parts.len() - 1 - tail]
    {
        tail += 1;
    }
    let join = |parts: &[&str]| parts.join("/");
    let prefix = join(&old_parts[..head]);
    let suffix = join(&old_parts[old_parts.len() - tail..]);
    let middle = format!(
        "{{{} => {}}}",
        join(&old_parts[head..old_parts.len() - tail]),
        join(&new_parts[head..new_parts.len() - tail])
    );
    let mut path = String::new();
    if !prefix.is_empty() {
        path.push_str(&prefix);
        path.push('/');
    }
    path.push_str(&middle);
    if !suffix.is_empty() {
        path.push('/');
        path.push_str(&suffix);
    }
    path
}

/// The directories a rename moves a file between, when its name stays.
pub(super) fn moved_between(old: &str, new: &str) -> Option<(String, String)> {
    let (old_dir, old_name) = old.rsplit_once('/').unwrap_or(("", old));
    let (new_dir, new_name) = new.rsplit_once('/').unwrap_or(("", new));
    (old_name == new_name && old_dir != new_dir).then(|| (old_dir.to_owned(), new_dir.to_owned()))
}

/// The deleted file most of whose lines `added` has, if any reaches
/// [`LIKELY_RENAME`]. Only whole files are compared: both sides' hunks hold
/// every line of an added or deleted file.
pub(super) fn likely_source<'a>(added: &FileDiff, deleted: &[&'a FileDiff]) -> Option<&'a str> {
    let lines = |hunks: &str, sign: char| -> Vec<String> {
        hunks
            .lines()
            .filter_map(|l| l.strip_prefix(sign))
            .map(str::trim)
            .filter(|l| l.len() > 3)
            .map(str::to_owned)
            .collect()
    };
    let new_lines = lines(&added.hunks, '+');
    if new_lines.is_empty() {
        return None;
    }
    let mut best: Option<(f64, &str)> = None;
    for old in deleted {
        let old_lines = lines(&old.hunks, '-');
        if old_lines.is_empty() {
            continue;
        }
        let shared = old_lines.iter().filter(|l| new_lines.contains(l)).count();
        let score = shared as f64 / old_lines.len().min(new_lines.len()) as f64;
        if score >= LIKELY_RENAME && best.is_none_or(|(s, _)| score > s) {
            best = Some((score, old.path.as_str()));
        }
    }
    best.map(|(_, path)| path)
}
