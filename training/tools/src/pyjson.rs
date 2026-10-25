//! JSON as Python's `json.dumps(..., ensure_ascii=False)` writes it, so the
//! files the tools write stay byte for byte what the Python tools wrote
//! (the training data is identified by their hashes).

use serde_json::Value;

/// `json.dumps(value, ensure_ascii=False)`: `", "` and `": "` as separators.
pub fn dumps(value: &Value) -> String {
    let mut out = String::new();
    write(value, &mut out, None, 0);
    out
}

/// `json.dumps(value, ensure_ascii=False, indent=n)`.
#[allow(dead_code)] // the score report uses it
pub fn dumps_indent(value: &Value, indent: usize) -> String {
    let mut out = String::new();
    write(value, &mut out, Some(indent), 0);
    out
}

fn write(value: &Value, out: &mut String, indent: Option<usize>, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match n.as_f64().filter(|_| n.is_f64()) {
            Some(f) => out.push_str(&float(f)),
            None => out.push_str(&n.to_string()),
        },
        Value::String(s) => string(s, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                separator(out, indent, depth + 1, n == 0);
                write(item, out, indent, depth + 1);
            }
            close(out, indent, depth, ']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (n, (key, item)) in map.iter().enumerate() {
                separator(out, indent, depth + 1, n == 0);
                string(key, out);
                out.push_str(": ");
                write(item, out, indent, depth + 1);
            }
            close(out, indent, depth, '}');
        }
    }
}

fn separator(out: &mut String, indent: Option<usize>, depth: usize, first: bool) {
    match indent {
        Some(n) => {
            if !first {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&" ".repeat(n * depth));
        }
        None if !first => out.push_str(", "),
        None => {}
    }
}

fn close(out: &mut String, indent: Option<usize>, depth: usize, bracket: char) {
    if let Some(n) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * depth));
    }
    out.push(bracket);
}

fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Python's `repr(float)`: the shortest text that reads back, with `.0` on
/// whole numbers.
pub fn float(f: f64) -> String {
    format!("{f:?}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn separators_and_escapes_are_python_s() {
        let value = json!({"a": [1, 2], "b": null, "c": "é\n\"\u{1}\u{7f}"});
        assert_eq!(
            dumps(&value),
            "{\"a\": [1, 2], \"b\": null, \"c\": \"é\\n\\\"\\u0001\u{7f}\"}"
        );
        assert_eq!(dumps(&json!({})), "{}");
        assert_eq!(dumps(&json!([])), "[]");
    }

    #[test]
    fn keys_keep_their_order() {
        let value: Value = serde_json::from_str(r#"{"z": 1, "a": {"y": 2, "b": 3}}"#).unwrap();
        assert_eq!(dumps(&value), r#"{"z": 1, "a": {"y": 2, "b": 3}}"#);
    }

    #[test]
    fn indented_output_matches_python() {
        let value = json!({"a": [1, {"b": []}], "c": 0.5});
        assert_eq!(
            dumps_indent(&value, 2),
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": []\n    }\n  ],\n  \"c\": 0.5\n}"
        );
    }

    #[test]
    fn floats_keep_their_point() {
        assert_eq!(dumps(&json!(1.0)), "1.0");
        assert_eq!(dumps(&json!(0.656)), "0.656");
        assert_eq!(dumps(&json!(3)), "3");
    }
}
