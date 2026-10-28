//! Notes added to prompts when retrying or avoiding messages.

use crate::message;
use crate::style::{Convention, Language};

use super::SanitizeError;

/// Asks for a message other than the subjects of `messages`.
pub fn avoid_note(messages: &[String]) -> Option<String> {
    if messages.is_empty() {
        return None;
    }
    let subjects: Vec<String> = messages
        .iter()
        .map(|m| {
            let subject: String = m
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(message::SUBJECT_MAX)
                .collect();
            format!("\"{subject}\"")
        })
        .collect();
    Some(format!(
        "The user turned down {}: {}. Write a different message that describes \
         the change better.",
        if messages.len() == 1 {
            "this message"
        } else {
            "these messages"
        },
        subjects.join("; ")
    ))
}

/// Tells the model why its previous reply was rejected.
pub fn retry_note(error: &SanitizeError, convention: Convention, language: Language) -> String {
    let reason = match error {
        SanitizeError::Empty => "it contained no commit message".to_owned(),
        SanitizeError::NotConventional(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            format!("\"{subject}\" does not follow `type(scope): description`")
        }
        SanitizeError::CopiedHistory(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            format!(
                "\"{subject}\" repeats an earlier commit of this repository; \
                 describe what this diff changes instead"
            )
        }
        SanitizeError::TurnedDown(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            format!("\"{subject}\" is a message the user turned down; write a different one")
        }
        SanitizeError::CutOff(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            format!("\"{subject}\" stops in the middle of a sentence; finish it")
        }
        SanitizeError::WrongLanguage(subject) => {
            let subject: String = subject.chars().take(message::SUBJECT_MAX).collect();
            let language = match language {
                Language::Portuguese => "Brazilian Portuguese",
                _ => "English",
            };
            format!("\"{subject}\" is not in {language}; write it in {language}")
        }
    };
    let format = match convention {
        Convention::Conventional => " in the `type(scope): description` format",
        _ => "",
    };
    format!(
        "Your previous reply was rejected because {reason}. \
         Reply again with only the commit message{format}."
    )
}
