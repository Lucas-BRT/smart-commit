//! The Conventional Commits type a reply starts with, as the probability a
//! model gives each candidate for its first token.

use serde::{Deserialize, Serialize};

/// The types a reply may start with.
pub const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "chore", "refactor", "test", "perf", "build", "ci", "style", "revert",
];

/// A type the reply may start with, and how likely the model finds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeChoice {
    pub name: String,
    pub probability: f64,
}

/// The type a token begins: the type itself (in any case), `tests` for
/// `test`, or the only type that starts with it, since a tokenizer may split
/// `refactor` into `ref` and `actor`. Two types sharing the start (`re`)
/// give none.
fn type_of(token: &str) -> Option<&'static str> {
    let token = token.trim().to_lowercase();
    if token == "tests" {
        return Some("test");
    }
    if let Some(known) = TYPES.iter().find(|t| **t == token) {
        return Some(known);
    }
    if token.chars().count() < 2 {
        return None;
    }
    let mut starts = TYPES.iter().filter(|t| t.starts_with(&token));
    match (starts.next(), starts.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// The types among `candidates` (a token and its log probability, as a
/// server lists them), most likely first, with probabilities adding up to 1
/// over the types found. Tokens of one type add up; the rest are ignored.
pub fn from_logprobs(candidates: &[(String, f64)]) -> Vec<TypeChoice> {
    let mut choices: Vec<TypeChoice> = Vec::new();
    for (token, logprob) in candidates {
        let Some(name) = type_of(token) else { continue };
        let probability = logprob.exp();
        match choices.iter_mut().find(|c| c.name == name) {
            Some(known) => known.probability += probability,
            None => choices.push(TypeChoice {
                name: name.to_owned(),
                probability,
            }),
        }
    }
    let total: f64 = choices.iter().map(|c| c.probability).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    for choice in &mut choices {
        choice.probability /= total;
    }
    // Ties by name, so the order never depends on the server's.
    choices.sort_by(|a, b| {
        b.probability
            .total_cmp(&a.probability)
            .then_with(|| a.name.cmp(&b.name))
    });
    choices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(list: &[(&str, f64)]) -> Vec<(String, f64)> {
        list.iter()
            .map(|(t, p)| ((*t).to_owned(), p.ln()))
            .collect()
    }

    #[test]
    fn types_are_found_among_other_tokens_and_normalised() {
        let choices = from_logprobs(&tokens(&[
            ("feat", 0.5),
            ("fix", 0.3),
            ("(", 0.1),
            ("chore", 0.1),
        ]));
        let names: Vec<&str> = choices.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["feat", "fix", "chore"]);
        let total: f64 = choices.iter().map(|c| c.probability).sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!((choices[0].probability - 0.5 / 0.9).abs() < 1e-9);
    }

    #[test]
    fn split_and_differently_spelt_tokens_count_for_their_type() {
        let choices = from_logprobs(&tokens(&[
            ("ref", 0.2),
            ("Fix", 0.3),
            ("fix", 0.1),
            ("ch", 0.1),
            ("tests", 0.05),
        ]));
        let find = |name: &str| {
            choices
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.probability)
        };
        let total = 0.2 + 0.4 + 0.1 + 0.05;
        assert!((find("fix").unwrap() - 0.4 / total).abs() < 1e-9);
        assert!((find("refactor").unwrap() - 0.2 / total).abs() < 1e-9);
        assert!((find("chore").unwrap() - 0.1 / total).abs() < 1e-9);
        assert!((find("test").unwrap() - 0.05 / total).abs() < 1e-9);
    }

    #[test]
    fn a_start_two_types_share_or_a_single_letter_is_no_type() {
        assert_eq!(type_of("re"), None);
        assert_eq!(type_of("f"), None);
        assert_eq!(type_of("hello"), None);
        assert!(from_logprobs(&tokens(&[("re", 0.5), ("(", 0.5)])).is_empty());
    }

    #[test]
    fn equal_probabilities_are_ordered_by_name() {
        let choices = from_logprobs(&tokens(&[("fix", 0.4), ("docs", 0.4), ("feat", 0.2)]));
        let names: Vec<&str> = choices.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["docs", "fix", "feat"]);
    }

    #[test]
    fn nothing_found_gives_nothing() {
        assert!(from_logprobs(&[]).is_empty());
    }
}
