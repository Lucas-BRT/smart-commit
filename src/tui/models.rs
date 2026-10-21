//! `smart-commit model` in a terminal: the recommended models with what is
//! installed and in use, the models from disk used before, the chosen one's
//! details, its download's progress, and a path prompt for a model on disk.

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Gauge, Paragraph, Row, Table, TableState, Wrap};

use super::{hints, is_interrupt, theme};
use crate::models::{self, Local, Model};

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    /// Download this model (not installed yet), then use it.
    Download(&'static Model),
    /// Use this model, which is installed.
    Use(&'static Model),
    /// Use a model file or directory from disk.
    UseFile(PathBuf),
}

/// One row of the list: what of the model is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub model: &'static Model,
    pub gguf: bool,
    pub openvino: bool,
}

/// A row for a model from disk used before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEntry {
    pub local: Local,
    /// model.gguf or model.dir points at it.
    pub in_use: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Browsing,
    /// Typing a path.
    Path(String),
    Downloading {
        model: &'static Model,
        bytes: u64,
    },
}

pub struct Picker {
    entries: Vec<Entry>,
    local: Vec<LocalEntry>,
    in_use: &'static str,
    /// model.gguf or model.dir pointing elsewhere, shown as a note.
    custom: Option<String>,
    selected: usize,
    mode: Mode,
    status: Option<(String, bool)>,
    home: Option<String>,
}

impl Picker {
    pub fn new(
        entries: Vec<Entry>,
        in_use: &'static str,
        custom: Option<String>,
        home: Option<String>,
    ) -> Self {
        let selected = entries
            .iter()
            .position(|e| e.model.id == in_use)
            .unwrap_or(0);
        Self {
            entries,
            local: Vec::new(),
            in_use,
            custom,
            selected,
            mode: Mode::Browsing,
            status: None,
            home,
        }
    }

    /// Adds the models from disk used before, after the recommended ones;
    /// the one in use is selected.
    pub fn with_local(mut self, local: Vec<LocalEntry>) -> Self {
        if let Some(i) = local.iter().position(|l| l.in_use) {
            self.selected = self.entries.len() + i;
        }
        self.local = local;
        self
    }

    /// Whether `model.name` runs on vulkan and cpu, with no GGUF file from
    /// disk in its place.
    fn registry_in_use(&self, model: &Model) -> bool {
        model.id == self.in_use
            && !self
                .local
                .iter()
                .any(|l| l.in_use && l.local.key() == "model.gguf")
    }

    /// The row after the models: a model from disk.
    fn file_row(&self) -> usize {
        self.entries.len() + self.local.len()
    }

    pub fn downloading(&self) -> bool {
        matches!(self.mode, Mode::Downloading { .. })
    }

    pub fn progress(&mut self, bytes: u64) {
        if let Mode::Downloading { bytes: b, .. } = &mut self.mode {
            *b = bytes;
        }
    }

    /// The download ended; on failure the picker shows why and goes on.
    pub fn download_failed(&mut self, error: String) {
        self.mode = Mode::Browsing;
        self.status = Some((error, true));
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        if is_interrupt(&key) {
            return Action::Quit;
        }
        match &mut self.mode {
            // A download cannot be interrupted halfway, except by Ctrl+C:
            // curl resumes it next time.
            Mode::Downloading { .. } => Action::None,
            Mode::Path(text) => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::Browsing;
                    Action::None
                }
                KeyCode::Backspace => {
                    text.pop();
                    Action::None
                }
                KeyCode::Char(c) => {
                    text.push(c);
                    Action::None
                }
                KeyCode::Enter => {
                    let path = expand(text.trim(), self.home.as_deref());
                    if path.exists() {
                        Action::UseFile(path)
                    } else {
                        self.status = Some((format!("{} does not exist", path.display()), true));
                        Action::None
                    }
                }
                _ => Action::None,
            },
            Mode::Browsing => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = self.selected.saturating_sub(1);
                    self.status = None;
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.file_row());
                    self.status = None;
                    Action::None
                }
                KeyCode::Char('f') => {
                    self.mode = Mode::Path(String::new());
                    Action::None
                }
                KeyCode::Enter if self.selected == self.file_row() => {
                    self.mode = Mode::Path(String::new());
                    Action::None
                }
                KeyCode::Enter if self.selected >= self.entries.len() => Action::UseFile(
                    self.local[self.selected - self.entries.len()]
                        .local
                        .path
                        .clone(),
                ),
                KeyCode::Enter => {
                    let entry = &self.entries[self.selected];
                    if entry.gguf {
                        Action::Use(entry.model)
                    } else {
                        self.mode = Mode::Downloading {
                            model: entry.model,
                            bytes: 0,
                        };
                        self.status = None;
                        Action::Download(entry.model)
                    }
                }
                _ => Action::None,
            },
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let [title, list, details, status, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length((self.entries.len() + self.local.len()) as u16 + 4),
            Constraint::Min(6),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        frame.render_widget(
            Paragraph::new(Line::styled(
                " Models: downloaded from Hugging Face, pinned and checked",
                theme::title(),
            )),
            title,
        );

        let header = Row::new(["model", "size", "status", "installed", ""]).style(theme::dim());
        let mut rows: Vec<Row> = self
            .entries
            .iter()
            .map(|e| {
                let installed = match (e.gguf, e.openvino) {
                    (true, true) => "gguf, openvino",
                    (true, false) => "gguf",
                    (false, true) => "openvino",
                    (false, false) => "-",
                };
                let mark = if self.registry_in_use(e.model) {
                    Span::styled("<- in use", theme::title())
                } else {
                    Span::raw("")
                };
                Row::new(vec![
                    Cell::from(e.model.id),
                    Cell::from(models::size(e.model.gguf.size)),
                    Cell::from(e.model.status.to_string()),
                    Cell::from(installed),
                    Cell::from(mark),
                ])
            })
            .collect();
        rows.extend(self.local.iter().map(|l| {
            let mark = if l.in_use {
                Span::styled("<- in use", theme::title())
            } else {
                Span::raw("")
            };
            Row::new(vec![
                Cell::from(l.local.name()),
                Cell::from(l.local.size().map_or("-".to_owned(), models::size)),
                Cell::from("unverified"),
                Cell::from(if l.local.key() == "model.dir" {
                    "openvino"
                } else {
                    "gguf"
                }),
                Cell::from(mark),
            ])
        }));
        rows.push(Row::new(vec![Cell::from("a model from disk")]));
        // Room for the names of files from disk, within 80 columns.
        let name_width = self
            .local
            .iter()
            .map(|l| l.local.name().chars().count())
            .max()
            .unwrap_or(0)
            .clamp(22, 30) as u16;
        let table = Table::new(
            rows,
            [
                // Every model is Apache-2.0 (models::tests): the licence is
                // in the details, the room goes to the ids.
                Constraint::Length(name_width),
                Constraint::Length(7),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Length(9),
            ],
        )
        .header(header)
        .row_highlight_style(theme::selected())
        .highlight_symbol("> ")
        .block(Block::bordered());
        let mut state = TableState::default().with_selected(Some(self.selected));
        frame.render_stateful_widget(table, list, &mut state);

        match &self.mode {
            Mode::Downloading { model, bytes } => {
                let total = model.gguf.size.max(1);
                let ratio = (*bytes as f64 / total as f64).min(1.0);
                let gauge = Gauge::default()
                    .block(Block::bordered().title(format!(
                        " Downloading {} from {} ",
                        model.name, model.gguf.repo
                    )))
                    .gauge_style(ratatui::style::Style::new().fg(theme::ACCENT))
                    .ratio(ratio)
                    .label(format!(
                        "{} of {} ({:.0}%)",
                        models::size(*bytes),
                        models::size(total),
                        ratio * 100.0
                    ));
                let [gauge_area, _] =
                    Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(details);
                frame.render_widget(gauge, gauge_area);
            }
            Mode::Path(text) => {
                let lines = vec![
                    Line::raw("A GGUF file runs on vulkan and cpu; an OpenVINO model directory on"),
                    Line::raw("npu, intel-gpu and openvino-cpu. smart-commit cannot check them."),
                    Line::raw(""),
                    Line::from(vec![
                        Span::styled("Path: ", theme::title()),
                        Span::raw(text.clone()),
                        Span::styled("_", theme::dim()),
                    ]),
                ];
                frame.render_widget(
                    Paragraph::new(lines).block(Block::bordered().title(" A model from disk ")),
                    details,
                );
            }
            Mode::Browsing => {
                frame.render_widget(self.details(), details);
            }
        }

        if let Some((text, error)) = &self.status {
            let style = if *error {
                ratatui::style::Style::new().fg(theme::BAD)
            } else {
                ratatui::style::Style::new().fg(theme::GOOD)
            };
            frame.render_widget(
                Paragraph::new(Line::styled(format!(" {text}"), style)),
                status,
            );
        } else if let Some(custom) = &self.custom {
            frame.render_widget(
                Paragraph::new(Line::styled(format!(" {custom}"), theme::dim())),
                status,
            );
        }
        let keys_line = match self.mode {
            Mode::Browsing => hints(&[
                ("Up/Down", "choose"),
                ("Enter", "use (downloads it if needed)"),
                ("f", "a file"),
                ("q", "quit"),
            ]),
            Mode::Path(_) => hints(&[("Enter", "use it"), ("Esc", "back")]),
            Mode::Downloading { .. } => hints(&[("Ctrl+C", "stop (resumed next time)")]),
        };
        frame.render_widget(Paragraph::new(keys_line), keys);
    }

    fn details(&self) -> Paragraph<'static> {
        if let Some(l) = self
            .selected
            .checked_sub(self.entries.len())
            .and_then(|i| self.local.get(i))
        {
            return Paragraph::new(vec![
                Line::styled("A model from disk, used before", theme::title()),
                Line::from(vec![
                    Span::styled(format!("{:<9}", "path"), theme::dim()),
                    Span::raw(l.local.path.display().to_string()),
                ]),
                Line::from(vec![
                    Span::styled(format!("{:<9}", "devices"), theme::dim()),
                    Span::raw(format!("{} ({})", l.local.devices(), l.local.key())),
                ]),
                Line::raw("smart-commit did not choose it and cannot check it."),
            ])
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(format!(" {} ", l.local.name())));
        }
        let Some(entry) = self.entries.get(self.selected) else {
            return Paragraph::new(vec![
                Line::raw("Use a GGUF file, or an OpenVINO model directory, from this machine."),
                Line::raw("Press Enter or f to type its path."),
            ])
            .block(Block::bordered().title(" A model from disk "));
        };
        let model = entry.model;
        let devices = if model.openvino.is_some() {
            "every device"
        } else {
            "vulkan and cpu (no OpenVINO version: not npu, intel-gpu or openvino-cpu)"
        };
        let field = |name: &'static str, value: String| {
            Line::from(vec![
                Span::styled(format!("{name:<9}"), theme::dim()),
                Span::raw(value),
            ])
        };
        Paragraph::new(vec![
            Line::styled(model.summary.to_owned(), theme::title()),
            field("devices", devices.to_owned()),
            field("source", format!("huggingface.co/{}", model.gguf.repo)),
            field(
                "pinned",
                format!(
                    "commit {}, SHA-256 {}",
                    &model.gguf.revision[..12],
                    &model.gguf.sha256[..16]
                ),
            ),
            field("licence", model.license.to_owned()),
        ])
        .wrap(Wrap { trim: false })
        .block(Block::bordered().title(format!(" {} ", model.name)))
    }
}

/// `text` as a path, with `~/` for `home`.
fn expand(text: &str, home: Option<&str>) -> PathBuf {
    match (text.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::{char, draw, key};

    fn picker() -> Picker {
        let entries = models::REGISTRY
            .iter()
            .map(|model| Entry {
                model,
                gguf: model.id != "qwen2.5-coder-7b",
                openvino: model.openvino.is_some(),
            })
            .collect();
        Picker::new(entries, "qwen2.5-coder-1.5b", None, Some("/home/me".into()))
    }

    #[test]
    fn lists_the_models_with_the_one_in_use_selected() {
        insta::assert_snapshot!(draw(80, 20, |f| picker().render(f)));
    }

    #[test]
    fn enter_uses_an_installed_model_or_downloads_it() {
        let mut picker = picker();
        assert_eq!(
            picker.on_key(key(KeyCode::Enter)),
            Action::Use(models::find("qwen2.5-coder-1.5b").unwrap())
        );
        picker.on_key(key(KeyCode::Down));
        picker.on_key(char('j'));
        let seven = models::find("qwen2.5-coder-7b").unwrap();
        assert_eq!(picker.on_key(key(KeyCode::Enter)), Action::Download(seven));
        assert!(picker.downloading());
        assert_eq!(picker.on_key(char('q')), Action::None, "not halfway");

        picker.progress(2_341_536_768);
        let screen = draw(80, 20, |f| picker.render(f));
        assert!(
            screen.contains("Downloading Qwen2.5-Coder-7B-Instruct"),
            "{screen}"
        );
        assert!(screen.contains("2.3 GB of 4.7 GB (50%)"), "{screen}");

        picker.download_failed("cannot download https://...: timeout".into());
        assert!(!picker.downloading());
        assert!(draw(80, 20, |f| picker.render(f)).contains("cannot download"));
        assert_eq!(picker.on_key(char('q')), Action::Quit);
    }

    #[test]
    fn the_details_follow_the_selection() {
        let mut picker = picker();
        picker.on_key(key(KeyCode::Down));
        let screen = draw(90, 20, |f| picker.render(f));
        assert!(screen.contains("Qwen2.5-Coder-0.5B-Instruct"), "{screen}");
        assert!(screen.contains("no OpenVINO version"), "{screen}");
        assert!(screen.contains("commit ebb2015119c9"), "{screen}");
        picker.on_key(key(KeyCode::Up));
        picker.on_key(key(KeyCode::Up));
        assert!(draw(90, 20, |f| picker.render(f)).contains("every device"));
    }

    #[test]
    fn a_fine_tune_shows_where_it_comes_from() {
        let mut picker = picker();
        let index = models::REGISTRY
            .iter()
            .position(|m| m.id == "smart-commit-0.5b-v0.1")
            .unwrap();
        for _ in 0..index {
            picker.on_key(key(KeyCode::Down));
        }
        let screen = draw(90, 20, |f| picker.render(f));
        assert!(screen.contains("> smart-commit-0.5b-v0.1"), "{screen}");
        assert!(screen.contains("smart-commit 0.5B v0.1"), "{screen}");
        assert!(
            screen.contains("huggingface.co/LucasBRT/smart-commit-0.5b-v0.1"),
            "{screen}"
        );
        assert!(screen.contains("commit 829633003d09"), "{screen}");
        assert!(screen.contains("licence  Apache-2.0"), "{screen}");
    }

    #[test]
    fn models_from_disk_used_before_are_listed_and_chosen_again() {
        let dir = tempfile::TempDir::new().unwrap();
        let used = dir.path().join("smart-commit-0.5b-v0.2-q4_k_m.gguf");
        let other = dir.path().join("other.gguf");
        std::fs::write(&used, "GGUF").unwrap();
        std::fs::write(&other, "GGUF").unwrap();
        let mut picker = picker().with_local(vec![
            LocalEntry {
                local: Local {
                    path: other.clone(),
                },
                in_use: false,
            },
            LocalEntry {
                local: Local { path: used.clone() },
                in_use: true,
            },
        ]);
        let screen = draw(90, 24, |f| picker.render(f));
        assert!(
            screen.contains("> smart-commit-0.5b-v0.2-q4_k_m 0 MB    unverified   gguf"),
            "the one in use is selected: {screen}"
        );
        assert!(screen.contains("  other   "), "{screen}");
        // The file replaces model.name on vulkan and cpu.
        assert_eq!(screen.matches("<- in use").count(), 1, "{screen}");
        assert!(
            screen.contains("A model from disk, used before"),
            "{screen}"
        );
        assert!(screen.contains("vulkan and cpu (model.gguf)"), "{screen}");
        assert_eq!(picker.on_key(key(KeyCode::Enter)), Action::UseFile(used));

        picker.on_key(key(KeyCode::Up));
        assert_eq!(picker.on_key(key(KeyCode::Enter)), Action::UseFile(other));
        picker.on_key(key(KeyCode::Down));
        picker.on_key(key(KeyCode::Down));
        assert!(draw(90, 24, |f| picker.render(f)).contains("> a model from disk"));
    }

    #[test]
    fn a_model_in_use_from_disk_elsewhere_keeps_the_registry_mark() {
        // An OpenVINO directory replaces the model only on its devices.
        let dir = tempfile::TempDir::new().unwrap();
        let picker = picker().with_local(vec![LocalEntry {
            local: Local {
                path: dir.path().to_path_buf(),
            },
            in_use: true,
        }]);
        let screen = draw(90, 24, |f| picker.render(f));
        assert_eq!(screen.matches("<- in use").count(), 2, "{screen}");
    }

    #[test]
    fn a_path_is_typed_checked_and_expanded() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("m.gguf");
        std::fs::write(&file, "GGUF").unwrap();

        let mut picker = picker();
        for _ in 0..5 {
            picker.on_key(key(KeyCode::Down));
        }
        assert!(draw(80, 20, |f| picker.render(f)).contains("> a model from disk"));
        picker.on_key(key(KeyCode::Enter));
        for c in "/nope".chars() {
            picker.on_key(char(c));
        }
        assert!(draw(80, 20, |f| picker.render(f)).contains("Path: /nope_"));
        assert_eq!(picker.on_key(key(KeyCode::Enter)), Action::None);
        assert!(draw(80, 20, |f| picker.render(f)).contains("/nope does not exist"));
        for _ in 0..5 {
            picker.on_key(key(KeyCode::Backspace));
        }
        for c in file.to_str().unwrap().chars() {
            picker.on_key(char(c));
        }
        assert_eq!(picker.on_key(key(KeyCode::Enter)), Action::UseFile(file));

        assert_eq!(
            expand("~/m.gguf", Some("/home/me")),
            PathBuf::from("/home/me/m.gguf")
        );
        assert_eq!(expand("~/m.gguf", None), PathBuf::from("~/m.gguf"));
        picker.on_key(key(KeyCode::Esc));
        assert_eq!(picker.on_key(char('f')), Action::None);
        assert_eq!(picker.on_key(key(KeyCode::Esc)), Action::None);
        assert_eq!(picker.on_key(char('q')), Action::Quit);
    }
}
