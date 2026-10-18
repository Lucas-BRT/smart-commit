//! `smart-commit bench` and `smart-commit device` in a terminal: each device
//! as it is measured (or as the last benchmark left it), what fast load
//! did, and choosing the device to use.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};

use super::{hints, is_interrupt, theme};
use crate::bench::{Entry, Report};
use crate::device::Target;

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    /// Use this device from now on.
    Use(Target),
    /// Back to the fastest device.
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Waiting,
    /// Being measured; what is happening.
    Running(&'static str),
    Done(Entry),
}

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

pub struct BenchView {
    title: String,
    rows: Vec<(Target, State)>,
    /// The device that will run, once known.
    in_use: Option<Target>,
    /// Whether model.device is auto.
    auto: bool,
    selected: usize,
    finished: bool,
    notes: Vec<String>,
    status: Option<String>,
    tick: usize,
}

impl BenchView {
    /// Before measuring `targets`.
    pub fn measuring(title: String, targets: &[Target], auto: bool) -> Self {
        Self {
            title,
            rows: targets.iter().map(|&t| (t, State::Waiting)).collect(),
            in_use: None,
            auto,
            selected: 0,
            finished: false,
            notes: Vec::new(),
            status: None,
            tick: 0,
        }
    }

    /// The last benchmark, finished.
    pub fn from_report(
        title: String,
        report: &Report,
        in_use: Option<Target>,
        auto: bool,
        notes: Vec<String>,
    ) -> Self {
        let mut view = Self {
            rows: report
                .entries
                .iter()
                .map(|e| (e.target, State::Done(e.clone())))
                .collect(),
            ..Self::measuring(title, &[], auto)
        };
        view.done(in_use, notes);
        view
    }

    pub fn start(&mut self, target: Target, what: &'static str) {
        if let Some(row) = self.rows.iter_mut().find(|(t, _)| *t == target) {
            row.1 = State::Running(what);
        }
    }

    pub fn finish(&mut self, entry: Entry) {
        if let Some(row) = self.rows.iter_mut().find(|(t, _)| *t == entry.target) {
            row.1 = State::Done(entry);
        }
    }

    /// Every device measured: shows which one will run and the notes, and
    /// lets the user choose.
    pub fn done(&mut self, in_use: Option<Target>, notes: Vec<String>) {
        self.finished = true;
        self.in_use = in_use;
        self.notes = notes;
        self.selected = self
            .rows
            .iter()
            .position(|(t, _)| Some(*t) == in_use)
            .unwrap_or(0);
        self.status = Some(match in_use {
            Some(target) => {
                let why = if self.auto {
                    "the fastest, model.device = auto"
                } else {
                    "set in model.device"
                };
                format!("smart-commit uses {target}: {why}.")
            }
            None => "No device could run the model: `smart-commit setup` installs what they need."
                .to_owned(),
        });
    }

    /// Ctrl+C while measuring: the device being measured finishes first.
    pub fn stopping(&mut self) {
        self.status = Some("Stopping after the device being measured...".to_owned());
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        if is_interrupt(&key) {
            return Action::Quit;
        }
        if !self.finished {
            return Action::None;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
                Action::None
            }
            KeyCode::Char('a') => Action::Auto,
            KeyCode::Enter => match self.rows.get(self.selected) {
                Some((target, State::Done(entry))) if entry.measurement.is_some() => {
                    Action::Use(*target)
                }
                Some((target, _)) => {
                    self.status = Some(format!(
                        "{target} could not run the model in the benchmark; choose another."
                    ));
                    Action::None
                }
                None => Action::None,
            },
            _ => Action::None,
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let [table_area, notes_area, status_area, keys_area] = Layout::vertical([
            Constraint::Length(self.rows.len() as u16 + 3),
            Constraint::Length(self.notes.len() as u16 + 1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let header =
            Row::new(["device", "per message", "load", "generate", "CPU", ""]).style(theme::dim());
        let seconds = |ms: f64| format!("{:.1} s", ms / 1000.0);
        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|(target, state)| {
                let name = Cell::from(target.name());
                match state {
                    State::Waiting => Row::new(vec![
                        name,
                        Cell::from(""),
                        Cell::from(""),
                        Cell::from(""),
                        Cell::from(""),
                        Cell::from("waiting"),
                    ])
                    .style(theme::dim()),
                    State::Running(what) => Row::new(vec![
                        name,
                        Cell::from(SPINNER[self.tick % SPINNER.len()]),
                        Cell::from(""),
                        Cell::from(""),
                        Cell::from(""),
                        Cell::from(*what),
                    ])
                    .style(Style::new().fg(theme::WARN)),
                    State::Done(entry) => match &entry.measurement {
                        Some(m) => {
                            let mut note = String::new();
                            if entry.fast_load.as_ref().is_some_and(|f| f.enabled) {
                                note.push_str("fast load");
                            }
                            if Some(*target) == self.in_use {
                                if !note.is_empty() {
                                    note.push_str(", ");
                                }
                                note.push_str("<- in use");
                            }
                            Row::new(vec![
                                name,
                                Cell::from(seconds(m.per_message_ms)),
                                Cell::from(seconds(m.load_ms)),
                                Cell::from(seconds(m.generate_ms)),
                                Cell::from(format!("{:.1}x", m.cpu_cores)),
                                Cell::from(note),
                            ])
                        }
                        None => {
                            let error = entry.error.as_deref().unwrap_or("unknown error");
                            let error = error.lines().next().unwrap_or(error);
                            Row::new(vec![
                                name,
                                Cell::from("-"),
                                Cell::from(""),
                                Cell::from(""),
                                Cell::from(""),
                                Cell::from(format!(
                                    "unavailable: {}",
                                    error.trim_start_matches("backend unavailable: ")
                                )),
                            ])
                            .style(Style::new().fg(theme::BAD))
                        }
                    },
                }
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(11),
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(5),
                Constraint::Fill(1),
            ],
        )
        .header(header)
        .block(Block::bordered().title(Line::styled(format!(" {} ", self.title), theme::title())));
        let table = if self.finished {
            table
                .row_highlight_style(theme::selected())
                .highlight_symbol("> ")
        } else {
            table
        };
        let mut state = TableState::default().with_selected(self.finished.then_some(self.selected));
        frame.render_stateful_widget(table, table_area, &mut state);

        let notes: Vec<Line> = self
            .notes
            .iter()
            .map(|n| Line::styled(format!(" {n}"), Style::new().fg(theme::GOOD)))
            .collect();
        frame.render_widget(Paragraph::new(notes), notes_area);
        if let Some(status) = &self.status {
            frame.render_widget(Paragraph::new(Line::raw(format!(" {status}"))), status_area);
        }
        let keys = if self.finished {
            hints(&[
                ("Up/Down", "choose"),
                ("Enter", "use this device"),
                ("a", "auto: the fastest"),
                ("q", "done"),
            ])
        } else {
            hints(&[("Ctrl+C", "stop")])
        };
        frame.render_widget(Paragraph::new(keys), keys_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{FastLoadTrial, Machine, Measurement};
    use crate::tui::testing::{char, draw, key};

    fn measured(target: Target, ms: f64, fast: bool) -> Entry {
        Entry {
            target,
            backend: Some("b".into()),
            measurement: Some(Measurement {
                load_ms: ms * 0.3,
                generate_ms: ms * 0.7,
                per_message_ms: ms,
                cpu_cores: 5.5,
                replies: vec![],
            }),
            error: None,
            fast_load: fast.then(|| FastLoadTrial::new(Some(ms * 1.5), Some(ms))),
        }
    }

    fn unavailable(target: Target) -> Entry {
        Entry {
            target,
            backend: None,
            measurement: None,
            error: Some("backend unavailable: OpenVINO GenAI not found\nmore".into()),
            fast_load: None,
        }
    }

    fn report() -> Report {
        Report {
            format: crate::bench::FORMAT,
            tool_version: "0".into(),
            machine: Machine {
                cpu: "AMD Ryzen 5 5500".into(),
                kernel: "7.2".into(),
            },
            entries: vec![
                unavailable(Target::IntelGpu),
                measured(Target::Vulkan, 2300.0, true),
                measured(Target::Cpu, 1900.0, true),
            ],
        }
    }

    #[test]
    fn shows_devices_as_they_are_measured() {
        let mut view = BenchView::measuring(
            "Benchmark on AMD Ryzen 5 5500".into(),
            &[Target::IntelGpu, Target::Vulkan, Target::Cpu],
            true,
        );
        view.finish(unavailable(Target::IntelGpu));
        view.start(Target::Vulkan, "measuring, with fast load");
        insta::assert_snapshot!(draw(80, 12, |f| view.render(f)));
        assert_eq!(
            view.on_key(key(KeyCode::Enter)),
            Action::None,
            "not before the end"
        );
        assert_eq!(view.on_key(char('q')), Action::None);
        view.stopping();
        assert!(draw(80, 12, |f| view.render(f)).contains("Stopping after the device"));
        assert_eq!(
            view.on_key(KeyEvent::new(
                KeyCode::Char('c'),
                ratatui::crossterm::event::KeyModifiers::CONTROL
            )),
            Action::Quit
        );
    }

    #[test]
    fn at_the_end_shows_the_choice_and_lets_the_user_change_it() {
        let report = report();
        let notes = report.fast_load_notes();
        let mut view = BenchView::from_report(
            "Last benchmark (AMD Ryzen 5 5500)".into(),
            &report,
            Some(Target::Cpu),
            true,
            notes,
        );
        insta::assert_snapshot!(draw(80, 12, |f| view.render(f)));
        view.on_key(key(KeyCode::Up));
        assert_eq!(
            view.on_key(key(KeyCode::Enter)),
            Action::Use(Target::Vulkan)
        );
        view.on_key(char('k'));
        assert_eq!(view.on_key(key(KeyCode::Enter)), Action::None);
        assert!(draw(80, 12, |f| view.render(f)).contains("intel-gpu could not run the model"));
        assert_eq!(view.on_key(char('a')), Action::Auto);
        assert_eq!(view.on_key(char('q')), Action::Quit);
    }

    #[test]
    fn says_when_nothing_could_run() {
        let mut view = BenchView::measuring("Benchmark".into(), &[Target::Npu], false);
        view.finish(unavailable(Target::Npu));
        view.done(None, vec![]);
        assert!(draw(80, 8, |f| view.render(f)).contains("No device could run the model"));
        let mut fixed = BenchView::measuring("Benchmark".into(), &[Target::Cpu], false);
        fixed.finish(measured(Target::Cpu, 1900.0, false));
        fixed.done(Some(Target::Cpu), vec![]);
        assert!(draw(80, 8, |f| fixed.render(f)).contains("set in model.device"));
    }
}
