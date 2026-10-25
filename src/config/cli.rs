//! Editing the configuration files: `config set`, `unset`, `init` and the
//! cleanup of values a version cannot read.

use std::path::Path;

use toml_edit::{DocumentMut, Item, Table};

use super::keys::{KEYS, Kind, Scope, key, parse, renamed};
use super::store::{Resolved, read_file};
use super::{ConfigError, Origin, Value};

/// Sets `name` to `text` in the file at `path`, keeping its comments and
/// layout; creates the file if needed.
pub fn set_in_file(
    path: &Path,
    origin: &Origin,
    name: &str,
    text: &str,
    home: Option<&str>,
) -> Result<Value, ConfigError> {
    let key = key(name)?;
    if key.scope == Scope::User && matches!(origin, Origin::Repo(_)) {
        return Err(ConfigError::UserOnly {
            key: name.to_owned(),
            path: path.display().to_string(),
        });
    }
    // Files get the current name, even when given the old one.
    let text = renamed(name, text).unwrap_or(text);
    let value = parse(key, text, home).map_err(|problem| ConfigError::InvalidValue {
        key: name.to_owned(),
        location: None,
        problem,
    })?;
    let mut doc = load_document(path)?;
    let item = match &value {
        Value::Text(t) => toml_edit::value(t.as_str()),
        Value::Count(n) => toml_edit::value(i64::try_from(*n).expect("small counts")),
    };
    // A file from `config init` has the key commented out under its
    // description: uncommenting that line keeps the two together.
    if !has_key(&doc, name)
        && let Some(text) = uncomment_template_line(&doc.to_string(), name, &item)
        && let Ok(edited) = text.parse::<DocumentMut>()
        && has_key(&edited, name)
    {
        save_document(path, &edited)?;
        return Ok(value);
    }
    match name.split_once('.') {
        Some((section, field)) => {
            let table = doc
                .entry(section)
                .or_insert_with(|| Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| ConfigError::InvalidValue {
                    key: section.to_owned(),
                    location: Some(path.display().to_string()),
                    problem: "expected a table".to_owned(),
                })?;
            table[field] = item;
        }
        None => doc[name] = item,
    }
    save_document(path, &doc)?;
    Ok(value)
}

fn has_key(doc: &DocumentMut, name: &str) -> bool {
    match name.split_once('.') {
        Some((section, field)) => doc
            .get(section)
            .and_then(Item::as_table)
            .is_some_and(|t| t.contains_key(field)),
        None => doc.contains_key(name),
    }
}

/// Replaces the template's `# field = default` line for `name` (inside its
/// `[section]`, or before any section for top-level keys) with the value.
fn uncomment_template_line(text: &str, name: &str, item: &Item) -> Option<String> {
    let (section, field) = match name.split_once('.') {
        Some((section, field)) => (Some(section), field),
        None => (None, name),
    };
    let commented = format!("# {field} = ");
    let mut current: Option<&str> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(header.trim());
        }
        if !replaced && current == section && line.starts_with(&commented) {
            lines.push(format!("{field} = {}", item.to_string().trim()));
            replaced = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    replaced.then(|| lines.join("\n") + "\n")
}

/// Removes `name` from the file at `path`. Returns whether it was there.
pub fn unset_in_file(path: &Path, name: &str) -> Result<bool, ConfigError> {
    key(name)?;
    if !path.exists() {
        return Ok(false);
    }
    let mut doc = load_document(path)?;
    let removed = match name.split_once('.') {
        Some((section, field)) => doc
            .get_mut(section)
            .and_then(Item::as_table_mut)
            .and_then(|table| table.remove(field))
            .is_some(),
        None => doc.remove(name).is_some(),
    };
    if removed {
        save_document(path, &doc)?;
    }
    Ok(removed)
}

/// Removes from the global file at `path` every value this version cannot
/// read, such as a `model.name` written by a newer build, so the defaults
/// apply. Returns what was removed, the old value in each message. Other
/// errors (unknown keys, bad syntax) are left for the user to fix.
pub fn set_aside_invalid(path: &Path, home: Option<&str>) -> Result<Vec<ConfigError>, ConfigError> {
    let origin = Origin::Global(path.to_owned());
    let mut removed = Vec::new();
    loop {
        match read_file(path, &origin, home) {
            Err(ConfigError::InvalidValue {
                key,
                location,
                problem,
            }) => {
                let error = ConfigError::InvalidValue {
                    key: key.clone(),
                    location,
                    problem,
                };
                // Never loop on a value that cannot be removed.
                if !unset_in_file(path, &key)? {
                    return Err(error);
                }
                removed.push(error);
            }
            Err(err) => return Err(err),
            Ok(_) => return Ok(removed),
        }
    }
}

fn load_document(path: &Path) -> Result<DocumentMut, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(ConfigError::Io(format!(
                "cannot read {}: {err}",
                path.display()
            )));
        }
    };
    text.parse()
        .map_err(|err: toml_edit::TomlError| ConfigError::Syntax {
            path: path.display().to_string(),
            problem: err.message().to_owned(),
        })
}

fn save_document(path: &Path, doc: &DocumentMut) -> Result<(), ConfigError> {
    let io =
        |err: std::io::Error| ConfigError::Io(format!("cannot write {}: {err}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, doc.to_string()).map_err(io)
}

/// A file listing every key allowed in `scope`'s file, all commented out,
/// with its description and default.
pub fn template(for_repo: bool, resolved_defaults: &Resolved) -> String {
    let mut out = String::from(if for_repo {
        "# smart-commit settings for this repository, shared through git.\n\
         # They override the global config; flags and environment variables\n\
         # override them. Uncomment a line to change it.\n"
    } else {
        "# smart-commit global settings. A repository's .smart-commit.toml overrides\n\
         # the style.* keys; flags and environment variables override both.\n\
         # `smart-commit config list --show-origin` shows where each value comes\n\
         # from. Uncomment a line to change it.\n"
    });
    let mut section = None;
    for key in KEYS {
        if for_repo && key.scope == Scope::User {
            continue;
        }
        let (current, field) = match key.name.split_once('.') {
            Some((s, f)) => (Some(s), f),
            None => (None, key.name),
        };
        if current != section {
            if let Some(s) = current {
                out.push_str(&format!("\n[{s}]\n"));
            }
            section = current;
        }
        out.push('\n');
        for line in wrap(key.description, 74) {
            out.push_str(&format!("# {line}\n"));
        }
        if let Kind::Choice(choices) = key.kind {
            out.push_str(&format!("# One of: {}.\n", choices.join(", ")));
        }
        if let Some(var) = key.env {
            out.push_str(&format!("# Environment: {var}.\n"));
        }
        let default = match resolved_defaults.get(key.name) {
            Some((Value::Text(t), _)) => format!("{t:?}"),
            Some((Value::Count(n), _)) => n.to_string(),
            None => "\"\"".to_owned(),
        };
        out.push_str(&format!("# {field} = {default}\n"));
    }
    out
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().expect("never empty");
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_owned());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}
