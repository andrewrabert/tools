use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

/// A single-line text input.
#[derive(Default)]
pub struct Field {
    text: String,
    /// Byte offset of the cursor, always on a char boundary.
    cursor: usize,
}

impl Field {
    pub fn new(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn handle(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('a') if control => self.cursor = 0,
            KeyCode::Char('e') if control => self.cursor = self.text.len(),
            KeyCode::Char('u') if control => {
                self.text.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char('k') if control => self.text.truncate(self.cursor),
            KeyCode::Char('w') if control => {
                let before = self.text[..self.cursor].trim_end();
                let start = before
                    .char_indices()
                    .rfind(|(_, c)| c.is_whitespace())
                    .map_or(0, |(index, c)| index + c.len_utf8());
                self.text.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Char(c) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.text.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Backspace => {
                if let Some(start) = self.previous() {
                    self.text.drain(start..self.cursor);
                    self.cursor = start;
                }
            }
            KeyCode::Delete => {
                if let Some(end) = self.next() {
                    self.text.drain(self.cursor..end);
                }
            }
            KeyCode::Left => self.cursor = self.previous().unwrap_or(self.cursor),
            KeyCode::Right => self.cursor = self.next().unwrap_or(self.cursor),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            _ => {}
        }
    }

    /// Draw in a bordered box, placing the terminal cursor when active.
    pub fn render(&self, frame: &mut Frame, area: Rect, title: &str, active: bool) {
        let color = if active { Color::Cyan } else { Color::DarkGray };
        let block = Block::bordered()
            .title(format!(" {title} "))
            .border_style(Style::new().fg(color));
        let inner = block.inner(area);
        let before =
            u16::try_from(Line::raw(&self.text[..self.cursor]).width()).unwrap_or(u16::MAX);
        let scroll = before.saturating_sub(inner.width.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(self.text.as_str())
                .scroll((0, scroll))
                .block(block),
            area,
        );
        if active {
            frame.set_cursor_position((inner.x + before - scroll, inner.y));
        }
    }

    fn previous(&self) -> Option<usize> {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
    }

    fn next(&self) -> Option<usize> {
        self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| self.cursor + c.len_utf8())
    }
}
