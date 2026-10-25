#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommentSyntax {
    /// `#`, with Python's docstrings.
    Python,
    /// `#` alone: shell, Ruby, TOML, YAML.
    Hash,
    /// `//` and `/* ... */`.
    Slash,
}

impl CommentSyntax {
    pub(super) fn of(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.').map_or("", |(_, e)| e);
        match extension {
            "py" | "pyi" => Some(Self::Python),
            "sh" | "bash" | "rb" | "toml" | "yaml" | "yml" | "pl" | "r" => Some(Self::Hash),
            "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "go" | "c" | "h" | "cc" | "cpp"
            | "hpp" | "java" | "kt" | "swift" | "cs" | "scala" | "dart" | "php" => {
                Some(Self::Slash)
            }
            _ => None,
        }
    }

    /// Whether every line is blank or a comment (true for none: a side
    /// with no line, as when comments are only added). Lines are read in
    /// order, so a docstring or a block comment spanning several changed
    /// lines counts whole.
    pub(super) fn only_comments(self, lines: &[&str]) -> bool {
        let mut open = false;
        for line in lines {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            let comment = match self {
                Self::Hash => text.starts_with('#'),
                Self::Python => {
                    let quotes = text.matches("\"\"\"").count() + text.matches("'''").count();
                    let docstring = open || text.starts_with("\"\"\"") || text.starts_with("'''");
                    if quotes % 2 == 1 {
                        open = !open;
                    }
                    text.starts_with('#') || docstring
                }
                Self::Slash => {
                    let comment = open
                        || text.starts_with("//")
                        || text.starts_with("/*")
                        || text.starts_with('*');
                    if text.contains("/*") && !text.contains("*/") {
                        open = true;
                    } else if text.contains("*/") {
                        open = false;
                    }
                    comment
                }
            };
            if !comment {
                return false;
            }
        }
        true
    }
}
