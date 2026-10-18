//! Full-screen terminal interfaces, with ratatui: reviewing a message before
//! committing it, choosing a model, the benchmark, the device and the setup.
//!
//! Every screen is a state with `render` and `on_key`, so tests draw it on a
//! `TestBackend` and press keys without a terminal. They are only shown
//! when both stdin and stdout are terminals: hooks, pipes and scripts get the
//! plain text output instead.

pub mod models;
pub mod review;

use std::io::{self, IsTerminal};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::{DefaultTerminal, Frame};

/// Whether a full-screen interface can be shown: stdin and stdout are
/// terminals, and the terminal is not `dumb`.
pub fn interactive() -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
}

/// The terminal in full-screen mode, restored when dropped (and on panic,
/// by ratatui's hook).
pub struct Screen {
    terminal: DefaultTerminal,
}

impl Screen {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            terminal: ratatui::try_init()?,
        })
    }

    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        self.terminal.draw(render).map(|_| ())
    }

    /// The next key press, waiting for it.
    pub fn key(&mut self) -> io::Result<KeyEvent> {
        loop {
            if let Some(key) = self.key_within(Duration::from_secs(3600))? {
                return Ok(key);
            }
        }
    }

    /// A key press within `timeout`, if any (for screens that animate).
    pub fn key_within(&mut self, timeout: Duration) -> io::Result<Option<KeyEvent>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => Ok(Some(key)),
            _ => Ok(None),
        }
    }

    /// Leaves full-screen mode to run `f` (an editor, `sudo`), then comes
    /// back.
    pub fn suspend<T>(&mut self, f: impl FnOnce() -> T) -> io::Result<T> {
        ratatui::restore();
        let result = f();
        self.terminal = ratatui::try_init()?;
        self.terminal.clear()?;
        Ok(result)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Ctrl+C, which raw mode turns into a key: every screen treats it as quit.
pub fn is_interrupt(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

/// The colours, shared by every screen.
pub mod theme {
    use super::*;

    pub const ACCENT: Color = Color::Cyan;
    pub const GOOD: Color = Color::Green;
    pub const WARN: Color = Color::Yellow;
    pub const BAD: Color = Color::Red;

    pub fn title() -> Style {
        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
    }

    pub fn dim() -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    pub fn selected() -> Style {
        Style::new().fg(Color::Black).bg(ACCENT)
    }
}

/// A line of key hints: `[Enter] commit  [e] edit ...`.
pub fn hints<'a>(keys: &[(&'a str, &'a str)]) -> Line<'a> {
    let mut spans = Vec::new();
    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(format!("[{key}]"), theme::title()));
        spans.push(Span::raw(format!(" {what}")));
    }
    Line::from(spans)
}

#[cfg(test)]
pub(crate) mod testing {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// What `render` draws on a `width` x `height` terminal, as text.
    pub fn draw(width: u16, height: u16, render: impl FnOnce(&mut ratatui::Frame)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(render).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub fn char(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }
}
