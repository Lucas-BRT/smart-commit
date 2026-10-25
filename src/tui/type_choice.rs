//! `smart-commit --commit` in a terminal, when the model is unsure of the
//! type: the types it finds likeliest, to choose one, or to leave the choice
//! to the model.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::{hints, is_interrupt, theme};
use crate::backend::TypeChoice;

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    /// This type, one of those offered.
    Chosen(String),
    /// None of them: the model picks, as it would have.
    ModelDecides,
    /// Give up on the commit.
    Cancel,
}

pub struct TypeChoiceView {
    choices: Vec<TypeChoice>,
    /// The highlighted line: a type, or the last, "let the model decide".
    selected: usize,
}

impl TypeChoiceView {
    /// `choices` are the types offered, most likely first; the first starts
    /// highlighted, since it is what the model would have written.
    pub fn new(choices: Vec<TypeChoice>) -> Self {
        Self {
            choices,
            selected: 0,
        }
    }

    /// The lines: every type, then leaving it to the model.
    fn lines(&self) -> usize {
        self.choices.len() + 1
    }

    fn outcome(&self, line: usize) -> Outcome {
        match self.choices.get(line) {
            Some(choice) => Outcome::Chosen(choice.name.clone()),
            None => Outcome::ModelDecides,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        if is_interrupt(&key) {
            return Outcome::Cancel;
        }
        match key.code {
            KeyCode::Enter => self.outcome(self.selected),
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.checked_sub(1).unwrap_or(self.lines() - 1);
                Outcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1) % self.lines();
                Outcome::None
            }
            KeyCode::Char(digit @ '1'..='9') => {
                let line = digit as usize - '1' as usize;
                if line < self.lines() {
                    self.outcome(line)
                } else {
                    Outcome::None
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => Outcome::Cancel,
            _ => Outcome::None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let height = self.lines() as u16 + 2;
        let [question, list, keys] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(height),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let top = &self.choices[0];
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(" Which type is this change?", theme::title()),
                Line::styled(
                    format!(
                        " The model is not sure: it would write {} ({:.0}%).",
                        top.name,
                        top.probability * 100.0
                    ),
                    theme::dim(),
                ),
            ]),
            question,
        );

        let mut lines: Vec<Line> = self
            .choices
            .iter()
            .enumerate()
            .map(|(n, choice)| {
                self.line(
                    n,
                    format!(
                        " {} {:<10} {:>3.0}%",
                        n + 1,
                        choice.name,
                        choice.probability * 100.0
                    ),
                )
            })
            .collect();
        lines.push(self.line(
            self.choices.len(),
            format!(" {} let the model decide", self.choices.len() + 1),
        ));
        frame.render_widget(Paragraph::new(lines).block(Block::bordered()), list);

        frame.render_widget(
            Paragraph::new(hints(&[
                ("Enter", "choose"),
                ("1-9", "pick"),
                ("↑/↓", "move"),
                ("q", "cancel"),
            ])),
            keys,
        );
    }

    fn line(&self, line: usize, text: String) -> Line<'static> {
        if line == self.selected {
            Line::from(Span::styled(text, theme::selected()))
        } else {
            Line::raw(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::{char, draw, key};
    use ratatui::crossterm::event::KeyModifiers;

    fn view() -> TypeChoiceView {
        TypeChoiceView::new(
            [("feat", 0.46), ("fix", 0.38), ("refactor", 0.16)]
                .iter()
                .map(|(name, probability)| TypeChoice {
                    name: (*name).to_owned(),
                    probability: *probability,
                })
                .collect(),
        )
    }

    #[test]
    fn offers_the_types_with_their_probabilities_and_leaving_it_to_the_model() {
        insta::assert_snapshot!(draw(60, 10, |f| view().render(f)));
    }

    #[test]
    fn enter_chooses_the_highlighted_line_which_starts_on_the_likeliest() {
        assert_eq!(
            view().on_key(key(KeyCode::Enter)),
            Outcome::Chosen("feat".into())
        );
        let mut view = view();
        assert_eq!(view.on_key(key(KeyCode::Down)), Outcome::None);
        assert_eq!(view.on_key(char('j')), Outcome::None);
        assert_eq!(
            view.on_key(key(KeyCode::Enter)),
            Outcome::Chosen("refactor".into())
        );
        // One more goes to leaving it to the model, and round again.
        view.on_key(key(KeyCode::Down));
        assert_eq!(view.on_key(key(KeyCode::Enter)), Outcome::ModelDecides);
        view.on_key(key(KeyCode::Down));
        assert_eq!(
            view.on_key(key(KeyCode::Enter)),
            Outcome::Chosen("feat".into())
        );
        assert_eq!(view.on_key(key(KeyCode::Up)), Outcome::None);
        assert_eq!(view.on_key(key(KeyCode::Enter)), Outcome::ModelDecides);
    }

    #[test]
    fn a_digit_chooses_its_line_at_once() {
        assert_eq!(view().on_key(char('2')), Outcome::Chosen("fix".into()));
        assert_eq!(view().on_key(char('3')), Outcome::Chosen("refactor".into()));
        assert_eq!(view().on_key(char('4')), Outcome::ModelDecides);
        assert_eq!(view().on_key(char('5')), Outcome::None);
        assert_eq!(view().on_key(char('0')), Outcome::None);
    }

    #[test]
    fn q_escape_and_interrupt_give_up() {
        assert_eq!(view().on_key(char('q')), Outcome::Cancel);
        assert_eq!(view().on_key(key(KeyCode::Esc)), Outcome::Cancel);
        let interrupt = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(view().on_key(interrupt), Outcome::Cancel);
        assert_eq!(view().on_key(char('x')), Outcome::None);
    }

    #[test]
    fn the_highlight_moves_on_the_screen() {
        let mut view = view();
        view.on_key(key(KeyCode::Down));
        let screen = draw(60, 10, |f| view.render(f));
        assert!(screen.contains(" 2 fix"), "{screen}");
        assert!(screen.contains("it would write feat (46%)"), "{screen}");
    }
}
