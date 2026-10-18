//! `smart-commit setup` in a terminal: the hardware, the plan with each step
//! on or off, then each step's progress and output.

use std::collections::VecDeque;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};

use super::{hints, is_interrupt, theme};
use crate::setup::{Step, StepKind};

/// Lines of output kept for the log.
const LOG_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Start,
    /// Open the model picker.
    ChangeModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Pending,
    Running,
    Done(String),
    Failed(String),
    Skipped(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Plan,
    Running,
    Finished,
}

pub struct SetupView {
    inventory: Vec<String>,
    not_needed: Option<&'static str>,
    steps: Vec<(Step, State)>,
    selected: usize,
    phase: Phase,
    log: VecDeque<String>,
    /// What is downloading, and how far.
    progress: Option<(String, f64)>,
    status: Option<String>,
    stopping: bool,
}

impl SetupView {
    pub fn new(inventory: &str, not_needed: Option<&'static str>, steps: Vec<Step>) -> Self {
        Self {
            inventory: inventory.lines().map(str::to_owned).collect(),
            not_needed,
            steps: steps.into_iter().map(|s| (s, State::Pending)).collect(),
            selected: 0,
            phase: Phase::Plan,
            log: VecDeque::new(),
            progress: None,
            status: None,
            stopping: false,
        }
    }

    /// The steps the user left on, in order.
    pub fn enabled(&self) -> Vec<StepKind> {
        self.steps
            .iter()
            .filter(|(s, _)| s.enabled)
            .map(|(s, _)| s.kind)
            .collect()
    }

    /// Every step's title and how it ended.
    pub fn results(&self) -> Vec<(String, State)> {
        self.steps
            .iter()
            .map(|(s, state)| (s.title.clone(), state.clone()))
            .collect()
    }

    pub fn step(&self, kind: StepKind) -> Option<&Step> {
        self.steps.iter().map(|(s, _)| s).find(|s| s.kind == kind)
    }

    /// Replaces a step, e.g. the model after the picker.
    pub fn replace(&mut self, step: Step) {
        if let Some(row) = self.steps.iter_mut().find(|(s, _)| s.kind == step.kind) {
            row.0 = step;
        }
    }

    /// Turns a step on, e.g. an installed model that must still be chosen.
    pub fn enable(&mut self, kind: StepKind) {
        if let Some(row) = self.steps.iter_mut().find(|(s, _)| s.kind == kind) {
            row.0.enabled = true;
        }
    }

    /// Removes a step that no longer applies (the export, for a model
    /// without an OpenVINO version).
    pub fn remove(&mut self, kind: StepKind) {
        self.steps.retain(|(s, _)| s.kind != kind);
        self.selected = self.selected.min(self.steps.len().saturating_sub(1));
    }

    pub fn set(&mut self, kind: StepKind, state: State) {
        if state == State::Running {
            self.phase = Phase::Running;
            self.progress = None;
            self.log.push_back(String::new());
            self.log.push_back(format!(
                "==> {}",
                self.step(kind).map_or("", |s| s.title.as_str())
            ));
        }
        if let Some(row) = self.steps.iter_mut().find(|(s, _)| s.kind == kind) {
            row.1 = state;
        }
    }

    pub fn log(&mut self, line: &str) {
        // Progress bars redraw a line with \r; only the last state matters.
        let Some(line) = line
            .split('\r')
            .map(str::trim_end)
            .rfind(|s| !s.trim().is_empty())
        else {
            return;
        };
        self.log.push_back(line.to_owned());
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    pub fn progress(&mut self, what: String, ratio: f64) {
        self.progress = Some((what, ratio.clamp(0.0, 1.0)));
    }

    pub fn stopping(&self) -> bool {
        self.stopping
    }

    pub fn finish(&mut self, summary: String) {
        self.phase = Phase::Finished;
        self.progress = None;
        self.status = Some(summary);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        if is_interrupt(&key) {
            if self.phase == Phase::Running {
                self.stopping = true;
                self.status = Some("Stopping after this step...".to_owned());
                return Action::None;
            }
            return Action::Quit;
        }
        match self.phase {
            Phase::Running => Action::None,
            Phase::Finished => match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => Action::Quit,
                _ => Action::None,
            },
            Phase::Plan => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = self.selected.saturating_sub(1);
                    Action::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.steps.len().saturating_sub(1));
                    Action::None
                }
                KeyCode::Char(' ') => {
                    if let Some((step, _)) = self.steps.get_mut(self.selected) {
                        step.enabled = !step.enabled;
                    }
                    Action::None
                }
                KeyCode::Char('m') => Action::ChangeModel,
                KeyCode::Enter => Action::Start,
                _ => Action::None,
            },
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let plan_height = self.steps.len() as u16 * 2 + 2 + u16::from(self.not_needed.is_some());
        let [hardware, plan, log, status_area, keys] = Layout::vertical([
            Constraint::Length(self.inventory.len() as u16 + 2),
            Constraint::Length(plan_height),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let inventory: Vec<Line> = self
            .inventory
            .iter()
            .map(|l| Line::raw(l.clone()))
            .collect();
        frame.render_widget(
            Paragraph::new(inventory).block(Block::bordered().title(Line::styled(
                " smart-commit setup: hardware ",
                theme::title(),
            ))),
            hardware,
        );

        let mut items: Vec<ListItem> = self
            .steps
            .iter()
            .map(|(step, state)| {
                let check = if step.enabled { "[x]" } else { "[ ]" };
                let (mark, style) = match state {
                    State::Pending if step.installed && !step.enabled => {
                        ("installed".to_owned(), theme::dim())
                    }
                    State::Pending => (String::new(), Style::new()),
                    State::Running => ("running".to_owned(), Style::new().fg(theme::WARN)),
                    State::Done(note) => (format!("done {note}"), Style::new().fg(theme::GOOD)),
                    State::Failed(why) => (format!("failed: {why}"), Style::new().fg(theme::BAD)),
                    State::Skipped(why) => (format!("skipped: {why}"), theme::dim()),
                };
                let sudo = if step.sudo { " (sudo)" } else { "" };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::raw(format!("{check} {}{sudo}  ", step.title)),
                        Span::styled(mark, style),
                    ]),
                    Line::styled(format!("    {}", step.detail), theme::dim()),
                ])
            })
            .collect();
        if let Some(note) = self.not_needed {
            items.push(ListItem::new(Line::styled(note, theme::dim())));
        }
        let list = List::new(items).block(Block::bordered().title(" Plan "));
        let list = if self.phase == Phase::Plan {
            list.highlight_style(theme::selected())
                .highlight_symbol("> ")
        } else {
            list
        };
        let mut state = ListState::default()
            .with_selected((self.phase == Phase::Plan).then_some(self.selected));
        frame.render_stateful_widget(list, plan, &mut state);

        let log_block = Block::bordered().title(" Output ");
        let inner = log_block.inner(log);
        frame.render_widget(log_block, log);
        let (gauge_area, text_area) = match &self.progress {
            Some(_) => {
                let [g, t] =
                    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
                (Some(g), t)
            }
            None => (None, inner),
        };
        if let (Some((what, ratio)), Some(area)) = (&self.progress, gauge_area) {
            frame.render_widget(
                Gauge::default()
                    .gauge_style(Style::new().fg(theme::ACCENT))
                    .ratio(*ratio)
                    .label(format!("{what} {:.0}%", ratio * 100.0)),
                area,
            );
        }
        let shown = text_area.height as usize;
        let lines: Vec<Line> = self
            .log
            .iter()
            .skip(self.log.len().saturating_sub(shown))
            .map(|l| Line::raw(l.clone()))
            .collect();
        frame.render_widget(Paragraph::new(lines), text_area);

        if let Some(status) = &self.status {
            frame.render_widget(Paragraph::new(Line::raw(format!(" {status}"))), status_area);
        }
        let keys_line = match self.phase {
            Phase::Plan => hints(&[
                ("Up/Down", "choose"),
                ("Space", "on/off"),
                ("m", "model"),
                ("Enter", "start"),
                ("q", "quit"),
            ]),
            Phase::Running => hints(&[("Ctrl+C", "stop after this step")]),
            Phase::Finished => hints(&[("Enter", "done")]),
        };
        frame.render_widget(Paragraph::new(keys_line), keys);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models;
    use crate::setup::{Facts, model_step, plan};
    use crate::tui::testing::{char, draw, key};

    const INVENTORY: &str = "  CPU  AMD Ryzen 5 5500 (x86_64)\n       -> cpu (llama.cpp)\n  \
                             GPU  NVIDIA device 2507 [0000:01:00.0], driver nvidia 595.104.02\n       \
                             -> vulkan (the NVIDIA driver's Vulkan ICD)\n  NPU  none\n";

    fn steps() -> Vec<Step> {
        let hw = crate::hardware::Hardware {
            cpu_vendor: crate::hardware::CpuVendor::Amd,
            cpu_model: "AMD Ryzen 5 5500".into(),
            arch: "x86_64".into(),
            gpus: vec![crate::hardware::Gpu {
                vendor: "0x10de".into(),
                device: "0x2507".into(),
                driver: "nvidia".into(),
                slot: "0000:01:00.0".into(),
                name: None,
                nvidia_version: Some("595.104.02".into()),
            }],
            npu: crate::hardware::Npu::None,
            openvino_installed: false,
        };
        plan(
            &hw,
            &Facts {
                model: models::find(models::DEFAULT).unwrap(),
                llama_installed: true,
                model_installed: false,
                npu_ready: false,
                exported: false,
                gpu_plan: vec![
                    "NVIDIA GPU (0000:01:00.0, proprietary driver 595.104.02): its own Vulkan ICD"
                        .into(),
                ],
                drivers: true,
                bench: true,
            },
        )
    }

    fn view() -> SetupView {
        SetupView::new(
            INVENTORY,
            Some("Not needed: OpenVINO, since there is no Intel NPU or GPU here."),
            steps(),
        )
    }

    #[test]
    fn shows_the_hardware_and_the_plan() {
        insta::assert_snapshot!(draw(90, 26, |f| view().render(f)));
    }

    #[test]
    fn steps_are_turned_on_and_off_before_starting() {
        let mut view = view();
        assert_eq!(
            view.enabled(),
            [StepKind::Model, StepKind::GpuDrivers, StepKind::Bench],
            "llama.cpp is installed"
        );
        view.on_key(key(KeyCode::Down));
        view.on_key(key(KeyCode::Down));
        view.on_key(char(' '));
        assert_eq!(view.enabled(), [StepKind::Model, StepKind::Bench]);
        view.on_key(key(KeyCode::Up));
        view.on_key(key(KeyCode::Up));
        view.on_key(char(' '));
        assert_eq!(
            view.enabled(),
            [StepKind::LlamaCpp, StepKind::Model, StepKind::Bench]
        );
        assert_eq!(view.on_key(char('m')), Action::ChangeModel);
        assert_eq!(view.on_key(key(KeyCode::Enter)), Action::Start);
        assert_eq!(view.on_key(char('q')), Action::Quit);
    }

    #[test]
    fn a_new_model_replaces_its_step() {
        let mut view = view();
        view.replace(model_step(models::find("qwen2.5-coder-7b").unwrap(), true));
        let screen = draw(90, 26, |f| view.render(f));
        assert!(screen.contains("Model qwen2.5-coder-7b"), "{screen}");
        assert!(screen.contains("installed"), "{screen}");
        assert!(!view.enabled().contains(&StepKind::Model));
        view.enable(StepKind::Model);
        assert!(view.enabled().contains(&StepKind::Model));
        view.remove(StepKind::Bench);
        assert!(!view.enabled().contains(&StepKind::Bench));
    }

    #[test]
    fn running_shows_states_output_and_progress() {
        let mut view = view();
        view.set(StepKind::Model, State::Running);
        view.progress("Qwen2.5-Coder-1.5B-Instruct: 560 MB of 1.1 GB".into(), 0.5);
        view.log("curl: downloading\r50%\r");
        let screen = draw(90, 26, |f| view.render(f));
        assert!(screen.contains("running"), "{screen}");
        assert!(screen.contains("==> Model qwen2.5-coder-1.5b"), "{screen}");
        assert!(screen.contains("560 MB of 1.1 GB 50%"), "{screen}");
        assert!(
            screen.contains("\n│50%"),
            "only the last state of a progress line: {screen}"
        );
        assert!(screen.contains("[Ctrl+C] stop after this step"), "{screen}");

        assert_eq!(view.on_key(char('q')), Action::None);
        let ctrl_c = KeyEvent::new(
            KeyCode::Char('c'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        );
        assert_eq!(view.on_key(ctrl_c), Action::None);
        assert!(view.stopping());

        view.set(StepKind::Model, State::Done("(checked)".into()));
        view.set(StepKind::GpuDrivers, State::Failed("exit status 1".into()));
        view.set(StepKind::Bench, State::Skipped("stopped".into()));
        view.finish("Done.".into());
        let results = view.results();
        assert_eq!(
            results[1],
            (
                "Model qwen2.5-coder-1.5b".to_owned(),
                State::Done("(checked)".into())
            )
        );
        assert_eq!(results[0].1, State::Pending, "llama.cpp was off");
        let screen = draw(90, 26, |f| view.render(f));
        assert!(screen.contains("done (checked)"), "{screen}");
        assert!(screen.contains("failed: exit status 1"), "{screen}");
        assert!(screen.contains("skipped: stopped"), "{screen}");
        assert_eq!(view.on_key(key(KeyCode::Enter)), Action::Quit);
    }

    #[test]
    fn the_log_keeps_the_last_lines() {
        let mut view = view();
        for i in 0..(LOG_LINES + 50) {
            view.log(&format!("line {i}"));
        }
        assert_eq!(view.log.len(), LOG_LINES);
        assert_eq!(
            view.log.back().unwrap(),
            &format!("line {}", LOG_LINES + 49)
        );
        view.log("   \r  ");
        assert_eq!(view.log.len(), LOG_LINES, "blank lines are dropped");
    }
}
