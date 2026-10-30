use super::FileDiff;

/// Parses `git diff --numstat -z` and the matching patch.
///
/// Both list files in the same order, so the n-th patch section belongs to
/// the n-th numstat entry; this avoids parsing (possibly quoted, possibly
/// prefix-less) paths out of the patch headers. If the two disagree, the
/// hunks are dropped and only the stat survives.
pub fn parse(numstat_z: &str, patch: &str) -> Vec<FileDiff> {
    let mut files = parse_numstat_z(numstat_z);
    let sections = split_sections(patch);
    if sections.len() == files.len() {
        for (file, section) in files.iter_mut().zip(sections) {
            file.hunks = hunks_of(section).to_owned();
        }
    }
    files
}

/// Parses `added\tdeleted\tpath\0`, or `added\tdeleted\t\0old\0new\0` for
/// renames and copies. Binary files have `-` counts.
pub(super) fn parse_numstat_z(out: &str) -> Vec<FileDiff> {
    let mut fields = out.split('\0');
    let mut files = Vec::new();
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let lines = added.parse().ok().zip(deleted.parse().ok());
        let (path, old_path) = if path.is_empty() {
            let (Some(old), Some(new)) = (fields.next(), fields.next()) else {
                break;
            };
            (new, Some(old.to_owned()))
        } else {
            (path, None)
        };
        files.push(FileDiff {
            path: path.to_owned(),
            old_path,
            lines,
            hunks: String::new(),
        });
    }
    files
}

/// Splits a patch at each `diff --git` (or `diff --cc` for conflicts).
pub(super) fn split_sections(patch: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = Vec::new();
    let mut offset = 0;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --") {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| &patch[start..starts.get(i + 1).copied().unwrap_or(patch.len())])
        .collect()
}

pub(super) fn hunks_of(section: &str) -> &str {
    if section.starts_with("@@") {
        return section;
    }
    section.find("\n@@").map_or("", |i| &section[i + 1..])
}
