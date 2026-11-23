use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use signal_hook::consts::{SIGINT, SIGTERM};

use crate::tools::base16_shell_preview::profile::Profile;
use crate::tools::base16_shell_preview::repo::Theme;

const COLORS: u8 = 22;
const ROWS: u16 = COLORS as u16;
const LIST_COLS: u16 = 35;
const SWATCH_COLS: u16 = 42;
const SIGNAL_POLL: Duration = Duration::from_millis(100);

enum Outcome<'a> {
    Enable(&'a Theme),
    Quit,
}

pub fn run(themes: &[Theme], profile: &Profile) -> Result<()> {
    if themes.is_empty() {
        bail!("no themes found");
    }
    let enabled = profile.enabled()?;
    let mut window = Window::new(themes.len(), usize::from(ROWS));
    if let Some(index) = enabled.as_ref().and_then(|enabled| {
        themes
            .iter()
            .position(|theme| theme.name() == enabled.name())
    }) {
        window.select(index);
    }

    let terminated = Arc::new(AtomicBool::new(false));
    for signal in [SIGINT, SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&terminated))
            .context("installing the signal handler")?;
    }

    let mut terminal = ratatui::try_init().context("starting the terminal UI")?;
    let outcome = browse(&mut terminal, themes, &mut window, &terminated);
    ratatui::restore();
    let restored = match outcome? {
        Outcome::Enable(theme) => {
            profile.enable(theme)?;
            Some(theme)
        }
        Outcome::Quit => enabled.as_ref(),
    };
    restored.map_or(Ok(()), Theme::apply)
}

fn browse<'a>(
    terminal: &mut DefaultTerminal,
    themes: &'a [Theme],
    window: &mut Window,
    terminated: &AtomicBool,
) -> Result<Outcome<'a>> {
    let mut applied = None;
    loop {
        check_size(terminal.size().context("reading the terminal size")?)?;
        terminal
            .draw(|frame| render(frame, themes, window))
            .context("drawing the terminal UI")?;
        if applied != Some(window.index()) {
            themes[window.index()].apply()?;
            applied = Some(window.index());
        }

        while !event::poll(SIGNAL_POLL).context("reading terminal input")? {
            if terminated.load(Ordering::Relaxed) {
                return Ok(Outcome::Quit);
            }
        }
        let Event::Key(key) = event::read().context("reading terminal input")? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Up => window.up(),
            KeyCode::Down => window.down(),
            KeyCode::PageUp => window.page_up(),
            KeyCode::PageDown => window.page_down(),
            KeyCode::Home => window.top(),
            KeyCode::End => window.bottom(),
            KeyCode::Char('q') | KeyCode::Esc => return Ok(Outcome::Quit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(Outcome::Quit);
            }
            KeyCode::Enter => return Ok(Outcome::Enable(&themes[window.index()])),
            _ => {}
        }
    }
}

fn check_size(size: Size) -> Result<()> {
    if size.height < ROWS {
        bail!("the terminal has fewer than {ROWS} lines");
    }
    let cols = LIST_COLS + SWATCH_COLS;
    if size.width < cols {
        bail!("the terminal has fewer than {cols} columns");
    }
    Ok(())
}

fn render(frame: &mut Frame, themes: &[Theme], window: &Window) {
    let width = usize::from(LIST_COLS - 1);
    let names = themes
        .iter()
        .skip(window.offset)
        .take(window.rows)
        .enumerate()
        .map(|(row, theme)| {
            let name: String = theme.name().chars().take(width).collect();
            let line = Line::raw(format!("{name:width$}"));
            if row == window.row {
                line.style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(names), Rect::new(0, 0, LIST_COLS, ROWS));

    let swatches = (0..COLORS)
        .map(|color| {
            let label = format!("color{color:02} ");
            let swatch = " ".repeat(usize::from(SWATCH_COLS - 1) - label.len());
            let style = Style::new().fg(Color::Indexed(color));
            Line::from(vec![
                Span::styled(label, style),
                Span::styled(swatch, style.add_modifier(Modifier::REVERSED)),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(swatches),
        Rect::new(LIST_COLS, 0, SWATCH_COLS, ROWS),
    );
}

#[derive(Debug, PartialEq)]
struct Window {
    len: usize,
    rows: usize,
    offset: usize,
    row: usize,
}

impl Window {
    fn new(len: usize, rows: usize) -> Self {
        Self {
            len,
            rows,
            offset: 0,
            row: 0,
        }
    }

    fn index(&self) -> usize {
        self.offset + self.row
    }

    fn select(&mut self, index: usize) {
        let index = index.min(self.len.saturating_sub(1));
        let current = self.index();
        if index < current {
            self.row -= (current - index).min(self.row);
        } else if index > current {
            let below = self.rows.saturating_sub(1 + self.row);
            self.row += (index - current).min(below);
        }
        self.offset = index - self.row;
    }

    fn up(&mut self) {
        self.select(self.index().saturating_sub(1));
    }

    fn down(&mut self) {
        self.select(self.index() + 1);
    }

    fn page_up(&mut self) {
        self.row = 0;
        self.select(self.index().saturating_sub(self.rows));
    }

    fn page_down(&mut self) {
        self.row = self.rows.saturating_sub(1);
        self.select(self.index() + self.rows);
    }

    fn top(&mut self) {
        self.select(0);
    }

    fn bottom(&mut self) {
        self.select(self.len.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(len: usize, rows: usize, offset: usize, row: usize) -> Window {
        Window {
            len,
            rows,
            offset,
            row,
        }
    }

    #[test]
    fn down_moves_the_row_then_scrolls() {
        let mut w = window(10, 3, 0, 1);
        w.down();
        assert_eq!(w, window(10, 3, 0, 2));
        w.down();
        assert_eq!(w, window(10, 3, 1, 2));
    }

    #[test]
    fn up_stops_at_the_top() {
        let mut w = window(10, 3, 0, 0);
        w.up();
        assert_eq!(w, window(10, 3, 0, 0));
        let mut w = window(10, 3, 4, 0);
        w.up();
        assert_eq!(w, window(10, 3, 3, 0));
    }

    #[test]
    fn pages_move_a_window_and_clamp() {
        let mut w = window(10, 3, 0, 0);
        w.page_down();
        assert_eq!(w, window(10, 3, 3, 2));
        w.page_down();
        assert_eq!(w, window(10, 3, 6, 2));
        w.page_down();
        assert_eq!(w, window(10, 3, 7, 2));
        w.page_up();
        assert_eq!(w, window(10, 3, 4, 0));
    }

    #[test]
    fn short_list_pages_within_the_window() {
        let mut w = window(2, 22, 0, 0);
        w.page_down();
        assert_eq!(w, window(2, 22, 0, 1));
        w.bottom();
        assert_eq!(w, window(2, 22, 0, 1));
        w.top();
        assert_eq!(w, window(2, 22, 0, 0));
    }
}
