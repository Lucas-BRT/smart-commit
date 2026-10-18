//! `smart-commit --commit` in a terminal: the message, where it came from
//! and what is staged, before committing it, editing it, asking for another
//! one or giving up.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::{hints, is_interrupt, theme};
use crate::generate::{Generation, Source};
use crate::git::{ChangeStatus, FileChange};
use crate::message::CommitMessage;

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Commit the message as it is.
    Commit,
    /// Open it in the editor, then commit.
    Edit,
    /// Ask the model for a different message.
    Another,
    Cancel,
}

pub struct Review {
    pub message: CommitMessage,
    source: Source,
    attempts: usize,
    /// Where the model ran, e.g. `qwen2.5-coder-1.5b on cpu`.
    running: String,
    /// Seconds the last generation took, loading included.
    seconds: f32,
    files: Vec<FileChange>,
    /// Every message shown before this one, which the user turned down.
    pub turned_down: Vec<String>,
    /// A one-line note: working, or what happened.
    status: Option<String>,
}

impl Review {
    pub fn new(
        generation: &Generation,
        running: String,
        seconds: f32,
        files: &[FileChange],
    ) -> Self {
        Self {
            message: generation.message.clone(),
            source: generation.source,
            attempts: generation.attempts.len(),
            running,
            seconds,
            files: files.to_vec(),
            turned_down: Vec::new(),
            status: None,
        }
    }

    /// Shown while the model writes another message.
    pub fn working(&mut self) {
        self.status = Some("Writing another message...".to_owned());
    }

    /// Replaces the message with the next one, noting when the model could
    /// only repeat itself.
    pub fn replace(&mut self, generation: &Generation, seconds: f32) {
        let text = generation.message.to_string();
        self.status = if self.turned_down.contains(&text) {
            Some("The model wrote a message it had already written.".to_owned())
        } else {
            None
        };
        self.message = generation.message.clone();
        self.source = generation.source;
        self.attempts = generation.attempts.len();
        self.seconds = seconds;
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        if is_interrupt(&key) {
            return Action::Cancel;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('c') => Action::Commit,
            KeyCode::Char('e') => Action::Edit,
            KeyCode::Char('a') | KeyCode::Char('r') => {
                self.turned_down.push(self.message.to_string());
                Action::Another
            }
            KeyCode::Char('q') | KeyCode::Esc => Action::Cancel,
            _ => Action::None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let files = self.files.len().min(6) as u16 + 2;
        let [message, info, staged, status, keys] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(files),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let mut text = Text::from(Line::styled(
            self.message.subject.clone(),
            theme::title().remove_modifier(ratatui::style::Modifier::DIM),
        ));
        if let Some(body) = &self.message.body {
            text.push_line(Line::raw(""));
            for line in body.lines() {
                text.push_line(Line::raw(line.to_owned()));
            }
        }
        let block = Block::bordered().title(Line::styled(" Commit message ", theme::title()));
        frame.render_widget(
            Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
            message,
        );

        let origin = match self.source {
            Source::Model if self.attempts > 1 => {
                format!("from the model, after {} attempts", self.attempts)
            }
            Source::Model => "from the model".to_owned(),
            Source::Fallback => {
                "built from the file list: the model's replies were unusable".to_owned()
            }
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!(" {} · {:.1} s · {origin}", self.running, self.seconds),
                theme::dim(),
            )),
            info,
        );

        let mut lines: Vec<Line> = self
            .files
            .iter()
            .take(5)
            .map(|f| {
                let path = match &f.old_path {
                    Some(old) => format!("{old} -> {}", f.path),
                    None => f.path.clone(),
                };
                Line::from(vec![
                    Span::styled(format!(" {} ", status_letter(&f.status)), theme::title()),
                    Span::raw(path),
                ])
            })
            .collect();
        if self.files.len() > 5 {
            lines.push(Line::styled(
                format!("   and {} more", self.files.len() - 5),
                theme::dim(),
            ));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(format!(" Staged: {} files ", self.files.len()))),
            staged,
        );

        if let Some(note) = &self.status {
            frame.render_widget(
                Paragraph::new(Line::styled(format!(" {note}"), theme::WARN)),
                status,
            );
        }
        frame.render_widget(
            Paragraph::new(hints(&[
                ("Enter", "commit"),
                ("e", "edit, then commit"),
                ("a", "another message"),
                ("q", "cancel"),
            ])),
            keys,
        );
    }
}

fn status_letter(status: &ChangeStatus) -> char {
    match status {
        ChangeStatus::Added => 'A',
        ChangeStatus::Modified => 'M',
        ChangeStatus::Deleted => 'D',
        ChangeStatus::Renamed => 'R',
        ChangeStatus::Copied => 'C',
        ChangeStatus::TypeChanged => 'T',
        ChangeStatus::Other(c) => *c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::{char, draw, key};

    fn generation(
        subject: &str,
        body: Option<&str>,
        source: Source,
        attempts: usize,
    ) -> Generation {
        Generation {
            message: CommitMessage {
                subject: subject.to_owned(),
                body: body.map(str::to_owned),
            },
            source,
            attempts: vec![
                crate::generate::Attempt {
                    prompt: crate::prompt::Prompt {
                        system: String::new(),
                        user: String::new(),
                    },
                    raw: String::new(),
                    rejected: None,
                };
                attempts
            ],
        }
    }

    fn change(status: ChangeStatus, path: &str) -> FileChange {
        FileChange {
            status,
            path: path.to_owned(),
            old_path: None,
        }
    }

    fn review() -> Review {
        Review::new(
            &generation(
                "feat(retry): add a retry loop",
                Some("Retries failed requests three times."),
                Source::Model,
                1,
            ),
            "qwen2.5-coder-1.5b on cpu".to_owned(),
            1.64,
            &[
                change(ChangeStatus::Modified, "src/retry.rs"),
                FileChange {
                    status: ChangeStatus::Renamed,
                    path: "src/net.rs".into(),
                    old_path: Some("src/http.rs".into()),
                },
            ],
        )
    }

    #[test]
    fn shows_the_message_its_origin_and_the_staged_files() {
        insta::assert_snapshot!(draw(72, 16, |f| review().render(f)));
    }

    #[test]
    fn says_when_the_message_is_the_fallback_and_shortens_long_file_lists() {
        let files: Vec<FileChange> = (0..8)
            .map(|i| change(ChangeStatus::Added, &format!("docs/page{i}.md")))
            .collect();
        let review = Review::new(
            &generation("docs: add 8 files", None, Source::Fallback, 2),
            "qwen2.5-coder-0.5b on vulkan".to_owned(),
            0.9,
            &files,
        );
        let screen = draw(80, 16, |f| review.render(f));
        assert!(screen.contains("built from the file list"), "{screen}");
        assert!(screen.contains("Staged: 8 files"), "{screen}");
        assert!(screen.contains("and 3 more"), "{screen}");
        assert!(!screen.contains("page5"), "{screen}");
    }

    #[test]
    fn keys_choose_what_happens() {
        let mut review = review();
        assert_eq!(review.on_key(key(KeyCode::Enter)), Action::Commit);
        assert_eq!(review.on_key(char('e')), Action::Edit);
        assert_eq!(review.on_key(char('q')), Action::Cancel);
        assert_eq!(review.on_key(key(KeyCode::Esc)), Action::Cancel);
        assert_eq!(
            review.on_key(KeyEvent::new(
                KeyCode::Char('c'),
                ratatui::crossterm::event::KeyModifiers::CONTROL
            )),
            Action::Cancel
        );
        assert_eq!(review.on_key(char('x')), Action::None);
        assert!(review.turned_down.is_empty());
    }

    #[test]
    fn another_message_remembers_the_ones_turned_down() {
        let mut review = review();
        assert_eq!(review.on_key(char('a')), Action::Another);
        assert_eq!(
            review.turned_down,
            ["feat(retry): add a retry loop\n\nRetries failed requests three times."]
        );
        review.working();
        assert!(draw(72, 16, |f| review.render(f)).contains("Writing another message..."));

        review.replace(
            &generation("fix(retry): stop after three tries", None, Source::Model, 2),
            0.4,
        );
        let screen = draw(72, 16, |f| review.render(f));
        assert!(
            screen.contains("fix(retry): stop after three tries"),
            "{screen}"
        );
        assert!(
            screen.contains("0.4 s · from the model, after 2 attempts"),
            "{screen}"
        );
        assert!(!screen.contains("Writing"), "{screen}");

        // The same message again is pointed out.
        review.on_key(char('a'));
        review.replace(
            &generation("fix(retry): stop after three tries", None, Source::Model, 1),
            0.4,
        );
        assert!(draw(72, 16, |f| review.render(f)).contains("already written"));
    }
}
