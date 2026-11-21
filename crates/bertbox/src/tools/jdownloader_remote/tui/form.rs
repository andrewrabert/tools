use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::tools::jdownloader_remote::tui::centered;

/// A single-line text input with a cursor counted in characters.
#[derive(Default)]
pub struct Input {
    text: String,
    cursor: usize,
}

impl Input {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn byte(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map_or(self.text.len(), |(index, _)| index)
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    /// Inserts pasted text, turning line breaks into spaces.
    pub fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            self.insert(if c == '\n' || c == '\r' { ' ' } else { c });
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let at = self.byte(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// Applies an editing key and reports whether the key was an editing key.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if control => self.clear(),
            KeyCode::Char('a') if control => self.home(),
            KeyCode::Char('e') if control => self.end(),
            KeyCode::Char(c) if !control => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            _ => return false,
        }
        true
    }
}

pub enum Value {
    Text(Input),
    Secret(Input),
    Toggle(bool),
    Choice { options: Vec<String>, index: usize },
}

pub struct Field {
    label: &'static str,
    value: Value,
}

impl Field {
    pub fn text(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: Value::Text(Input::new(value)),
        }
    }

    pub fn secret(label: &'static str) -> Self {
        Self {
            label,
            value: Value::Secret(Input::default()),
        }
    }

    pub fn toggle(label: &'static str, on: bool) -> Self {
        Self {
            label,
            value: Value::Toggle(on),
        }
    }

    pub fn choice(label: &'static str, options: Vec<String>, index: usize) -> Self {
        Self {
            label,
            value: Value::Choice { options, index },
        }
    }
}

pub enum FormEvent {
    Pending,
    Submit,
    Cancel,
}

pub struct Form {
    title: String,
    fields: Vec<Field>,
    focus: usize,
}

impl Form {
    pub fn new(title: impl Into<String>, fields: Vec<Field>) -> Self {
        Self {
            title: title.into(),
            fields,
            focus: 0,
        }
    }

    fn field(&self, label: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| &field.value)
    }

    /// The trimmed text of a text or secret field, empty when the field is missing.
    pub fn text(&self, label: &str) -> &str {
        match self.field(label) {
            Some(Value::Text(input) | Value::Secret(input)) => input.text().trim(),
            _ => "",
        }
    }

    /// The untrimmed text of a text or secret field.
    pub fn raw(&self, label: &str) -> &str {
        match self.field(label) {
            Some(Value::Text(input) | Value::Secret(input)) => input.text(),
            _ => "",
        }
    }

    pub fn optional(&self, label: &str) -> Option<String> {
        let text = self.text(label);
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub fn toggle(&self, label: &str) -> bool {
        matches!(self.field(label), Some(Value::Toggle(true)))
    }

    pub fn choice(&self, label: &str) -> Option<usize> {
        match self.field(label) {
            Some(Value::Choice { index, .. }) => Some(*index),
            _ => None,
        }
    }

    fn step(&mut self, by: isize) {
        let len = self.fields.len();
        if len > 0 {
            self.focus = (self.focus as isize + by).rem_euclid(len as isize) as usize;
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(Field {
            value: Value::Text(input) | Value::Secret(input),
            ..
        }) = self.fields.get_mut(self.focus)
        {
            input.insert_str(text);
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> FormEvent {
        match key.code {
            KeyCode::Esc => return FormEvent::Cancel,
            KeyCode::Enter => return FormEvent::Submit,
            KeyCode::Tab | KeyCode::Down => self.step(1),
            KeyCode::BackTab | KeyCode::Up => self.step(-1),
            _ => {
                let Some(field) = self.fields.get_mut(self.focus) else {
                    return FormEvent::Pending;
                };
                match &mut field.value {
                    Value::Text(input) | Value::Secret(input) => {
                        input.key(key);
                    }
                    Value::Toggle(on) => {
                        if matches!(
                            key.code,
                            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right
                        ) {
                            *on = !*on;
                        }
                    }
                    Value::Choice { options, index } => {
                        let len = options.len().max(1);
                        match key.code {
                            KeyCode::Right | KeyCode::Char(' ') => *index = (*index + 1) % len,
                            KeyCode::Left => *index = (*index + len - 1) % len,
                            _ => {}
                        }
                    }
                }
            }
        }
        FormEvent::Pending
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let width = area.width.saturating_sub(4).min(90);
        let height = u16::try_from(self.fields.len()).unwrap_or(u16::MAX) + 4;
        let area = centered(area, width, height);
        frame.render_widget(Clear, area);
        let block = Block::bordered().title(format!(" {} ", self.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let label_width = self
            .fields
            .iter()
            .map(|field| field.label.chars().count())
            .max()
            .unwrap_or_default()
            + 2;
        let value_width = usize::from(inner.width)
            .saturating_sub(label_width + 1)
            .max(1);
        let mut lines = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            let focused = index == self.focus;
            let label = format!("{:>width$}: ", field.label, width = label_width - 2);
            let label_style = if focused {
                Style::new().bold()
            } else {
                Style::new().dim()
            };
            let (value, cursor) = match &field.value {
                Value::Text(input) => window(input.text(), input.cursor(), value_width),
                Value::Secret(input) => {
                    let masked = "•".repeat(input.text().chars().count());
                    window(&masked, input.cursor(), value_width)
                }
                Value::Toggle(on) => (if *on { "[x]" } else { "[ ]" }.to_owned(), None),
                Value::Choice { options, index } => (
                    format!("◂ {} ▸", options.get(*index).map_or("", String::as_str)),
                    None,
                ),
            };
            if focused && let Some(cursor) = cursor {
                let x = inner.x + u16::try_from(label_width + cursor).unwrap_or(u16::MAX);
                let y = inner.y + u16::try_from(index).unwrap_or(u16::MAX);
                frame.set_cursor_position(Position::new(x.min(inner.right()), y));
            }
            let value_style = if focused {
                Style::new().reversed()
            } else {
                Style::new()
            };
            lines.push(Line::from(vec![
                Span::styled(label, label_style),
                Span::styled(value, value_style),
            ]));
        }
        lines.push(Line::default());
        lines.push(Line::from(
            "Tab/↑↓ field · ←→/Space change · Enter submit · Esc cancel".dim(),
        ));
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// The part of `text` that fits `width` columns with the cursor in view, and the cursor's column.
fn window(text: &str, cursor: usize, width: usize) -> (String, Option<usize>) {
    let start = (cursor + 1).saturating_sub(width);
    let visible: String = text.chars().skip(start).take(width).collect();
    (visible, Some(cursor - start))
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyEvent;

    use super::*;

    #[test]
    fn input_edits_at_the_cursor() {
        let mut input = Input::new("héllo");
        input.left();
        input.left();
        input.insert('X');
        assert_eq!(input.text(), "hélXlo");
        input.backspace();
        input.backspace();
        assert_eq!(input.text(), "hélo");
        input.home();
        input.delete();
        assert_eq!(input.text(), "élo");
        input.end();
        input.insert_str("a\nb");
        assert_eq!(input.text(), "éloa b");
        assert_eq!(input.cursor(), 6);
    }

    #[test]
    fn input_clears_with_control_u() {
        let mut input = Input::new("text");
        input.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn form_reads_fields_by_label() {
        let mut form = Form::new(
            "test",
            vec![
                Field::text("Name", "  value  "),
                Field::toggle("Start", true),
                Field::choice("Mode", vec!["a".into(), "b".into()], 0),
            ],
        );
        assert_eq!(form.text("Name"), "value");
        assert_eq!(form.optional("Missing"), None);
        assert!(form.toggle("Start"));
        form.key(KeyEvent::from(KeyCode::Tab));
        form.key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(!form.toggle("Start"));
        form.key(KeyEvent::from(KeyCode::Tab));
        form.key(KeyEvent::from(KeyCode::Left));
        assert_eq!(form.choice("Mode"), Some(1));
    }

    #[test]
    fn window_keeps_the_cursor_visible() {
        assert_eq!(window("abcdef", 6, 4), ("def".to_owned(), Some(3)));
        assert_eq!(window("abcdef", 1, 4), ("abcd".to_owned(), Some(1)));
    }
}
