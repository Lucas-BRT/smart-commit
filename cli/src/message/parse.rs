//! Parsing and cleaning model output into a subject and body.

/// Labels the model puts before the message, compared lowercase.
const LABELS: &[&str] = &[
    "suggested commit message",
    "commit message",
    "mensagem de commit sugerida",
    "mensagem de commit",
    "mensagem do commit",
    "mensagem",
    "subject",
    "assunto",
    "título",
    "titulo",
];
/// Openers of a whole preamble line such as "Here is the commit message:".
const PREAMBLES: &[&str] = &[
    "here is",
    "here's",
    "sure",
    "segue",
    "aqui está",
    "aqui esta",
];
/// Openers of an explanation after the message; everything from there on
/// is dropped.
const EXPLANATIONS: &[&str] = &[
    "explanation",
    "this commit message",
    "this message",
    "the commit message",
    "note:",
    "explicação",
    "explicacao",
    "esta mensagem",
    "essa mensagem",
    "nota:",
    "obs:",
];

/// Drops `<think>…</think>`; an unclosed block swallows the rest.
pub fn strip_thinking(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The contents of the first code fence, or the text if there is none.
pub fn unfence(text: &str) -> String {
    let is_fence = |l: &str| l.trim_start().starts_with("```");
    if !text.lines().any(is_fence) {
        return text.to_owned();
    }
    let inside: Vec<&str> = text
        .lines()
        .skip_while(|l| !is_fence(l))
        .skip(1)
        .take_while(|l| !is_fence(l))
        .collect();
    if inside.iter().any(|l| !l.trim().is_empty()) {
        inside.join("\n")
    } else {
        // An empty fence: the message was probably written around it.
        text.lines()
            .filter(|l| !is_fence(l))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn is_preamble(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    let Some(head) = lower.strip_suffix(':') else {
        return false;
    };
    PREAMBLES.iter().any(|p| head.starts_with(p)) || LABELS.contains(&head.trim_matches('*'))
}

pub fn is_explanation(line: &str) -> bool {
    let lower = line.trim().trim_start_matches(['*', '_']).to_lowercase();
    EXPLANATIONS.iter().any(|e| lower.starts_with(e))
}

pub fn clean_subject(line: &str) -> String {
    let mut s = line.trim();
    s = s.trim_start_matches('#').trim_start();
    s = strip_list_marker(s);
    s = strip_label(s);
    s = unwrap_pairs(s);
    let mut subject = s.split_whitespace().collect::<Vec<_>>().join(" ");
    while subject.ends_with('.') && !subject.ends_with("..") {
        subject.pop();
    }
    subject
}

/// `- x`, `* x`, `1. x`, `2) x`.
fn strip_list_marker(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")) {
        return rest.trim_start();
    }
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = s[digits..]
            .strip_prefix(". ")
            .or_else(|| s[digits..].strip_prefix(") "))
    {
        return rest.trim_start();
    }
    s
}

/// `Commit message: fix: x` → `fix: x`, also when the label is bold.
fn strip_label(s: &str) -> &str {
    let Some((head, rest)) = s.split_once(':') else {
        return s;
    };
    let label = head.trim().trim_matches('*').trim().to_lowercase();
    if LABELS.contains(&label.as_str()) {
        rest.trim_start_matches('*').trim()
    } else {
        s
    }
}

/// Repeatedly removes matching quotes, backticks or bold markers around
/// the whole subject.
fn unwrap_pairs(mut s: &str) -> &str {
    const PAIRS: &[(&str, &str)] = &[
        ("**", "**"),
        ("\"", "\""),
        ("'", "'"),
        ("`", "`"),
        ("“", "”"),
        ("‘", "’"),
    ];
    loop {
        let unwrapped = PAIRS.iter().find_map(|(open, close)| {
            s.strip_prefix(open)
                .and_then(|r| r.strip_suffix(close))
                .filter(|inner| !inner.trim().is_empty())
        });
        match unwrapped {
            Some(inner) => s = inner.trim(),
            None => return s,
        }
    }
}
