use std::path::{self, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Row, Table, TableState};
use ratatui::{DefaultTerminal, Frame};

use crate::tools::audio_tag::dump;
use crate::tools::audio_tag::field::Field;
use crate::tools::audio_tag::tags::Tags;

const TICK: Duration = Duration::from_millis(250);
const NOTICE_TIME: Duration = Duration::from_secs(3);
const QUIT_TIME: Duration = Duration::from_secs(3);
const PAGE: isize = 10;
/// Joins the values of a multi-valued tag while editing.
const SEPARATOR: &str = "; ";

pub fn run(path: &Path) -> Result<()> {
    let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    let (root, files) = if path.is_file() {
        let root = path.parent().unwrap_or(&path).to_path_buf();
        (root, vec![path])
    } else {
        let files = dump::audio_files(&path)?;
        (path, files)
    };
    if files.is_empty() {
        bail!("no audio files found in {}", root.display());
    }
    let entries = files
        .into_iter()
        .map(|path| {
            let tags = Tags::read(&path)?;
            let label = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            Ok(Entry {
                path,
                label,
                tags,
                modified: false,
            })
        })
        .collect::<Result<_>>()?;

    let mut app = App::new(root, entries);
    let mut terminal = ratatui::try_init().context("starting the terminal UI")?;
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}

struct Entry {
    path: PathBuf,
    label: String,
    tags: Tags,
    modified: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Files,
    Tags,
}

enum Modal {
    Edit {
        name: String,
        original: String,
        multiple: bool,
        field: Field,
    },
    Add {
        name: Field,
        value: Field,
        on_value: bool,
    },
}

#[derive(Clone, Copy)]
enum Level {
    Info,
    Warning,
    Error,
}

struct Notice {
    text: String,
    level: Level,
    until: Instant,
}

struct App {
    root: PathBuf,
    entries: Vec<Entry>,
    files: ListState,
    tags: TableState,
    focus: Focus,
    modal: Option<Modal>,
    notice: Option<Notice>,
    quit_armed: Option<Instant>,
    done: bool,
}

impl App {
    fn new(root: PathBuf, entries: Vec<Entry>) -> Self {
        let mut app = Self {
            root,
            entries,
            files: ListState::default().with_selected(Some(0)),
            tags: TableState::default(),
            focus: Focus::Files,
            modal: None,
            notice: None,
            quit_armed: None,
            done: false,
        };
        app.reset_tag_selection();
        app
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.done {
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(TICK)?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.key(key);
            }
            self.expire();
        }
        Ok(())
    }

    fn key(&mut self, key: KeyEvent) {
        if let Some(modal) = self.modal.take() {
            self.modal_key(modal, key);
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('c') if control => self.quit(),
            KeyCode::Char('a') => self.open_add(),
            KeyCode::Char('x') | KeyCode::Delete => self.delete_tag(),
            KeyCode::Char('s') => self.save(),
            KeyCode::Char('S') => self.save_all(),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Files => Focus::Tags,
                    Focus::Tags => Focus::Files,
                };
            }
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Files,
            KeyCode::Right | KeyCode::Char('l') => self.focus = Focus::Tags,
            KeyCode::Enter => match self.focus {
                Focus::Files => self.focus = Focus::Tags,
                Focus::Tags => self.open_edit(),
            },
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-PAGE),
            KeyCode::PageDown => self.move_by(PAGE),
            KeyCode::Home | KeyCode::Char('g') => self.move_by(isize::MIN),
            KeyCode::End | KeyCode::Char('G') => self.move_by(isize::MAX),
            _ => {}
        }
    }

    fn modal_key(&mut self, mut modal: Modal, key: KeyEvent) {
        match (&mut modal, key.code) {
            (_, KeyCode::Esc) => {}
            (
                Modal::Edit {
                    name,
                    original,
                    multiple,
                    field,
                },
                KeyCode::Enter,
            ) => {
                if field.text() != original {
                    let values = if *multiple {
                        field.text().split(SEPARATOR).map(str::to_owned).collect()
                    } else {
                        vec![field.text().to_owned()]
                    };
                    self.update_tags(|tags| tags.set(name, values));
                }
            }
            (Modal::Add { on_value, .. }, KeyCode::Tab | KeyCode::BackTab) => {
                *on_value = !*on_value;
                self.modal = Some(modal);
            }
            (Modal::Add { on_value, .. }, KeyCode::Enter) if !*on_value => {
                *on_value = true;
                self.modal = Some(modal);
            }
            (Modal::Add { name, value, .. }, KeyCode::Enter) => {
                let name = name.text().trim().to_lowercase();
                if !name.is_empty() {
                    let values = vec![value.text().to_owned()];
                    self.update_tags(|tags| {
                        tags.set(&name, values);
                        *tags = std::mem::take(tags).sorted();
                    });
                }
            }
            (Modal::Edit { field, .. }, _) => {
                field.handle(key);
                self.modal = Some(modal);
            }
            (
                Modal::Add {
                    name,
                    value,
                    on_value,
                },
                _,
            ) => {
                (if *on_value { value } else { name }).handle(key);
                self.modal = Some(modal);
            }
        }
    }

    fn current(&self) -> Option<&Entry> {
        self.files
            .selected()
            .and_then(|index| self.entries.get(index))
    }

    fn current_tag(&self) -> Option<(&str, &[String])> {
        let entry = self.current()?;
        entry.tags.iter().nth(self.tags.selected()?)
    }

    fn open_edit(&mut self) {
        let Some((name, values)) = self.current_tag() else {
            return;
        };
        let original = values.join(SEPARATOR);
        self.modal = Some(Modal::Edit {
            name: name.to_owned(),
            multiple: values.len() > 1,
            field: Field::new(original.clone()),
            original,
        });
    }

    fn open_add(&mut self) {
        if self.current().is_some() {
            self.modal = Some(Modal::Add {
                name: Field::default(),
                value: Field::default(),
                on_value: false,
            });
        }
    }

    fn delete_tag(&mut self) {
        let Some((name, _)) = self.current_tag() else {
            return;
        };
        let name = name.to_owned();
        self.update_tags(|tags| {
            tags.remove(&name);
        });
    }

    /// Change the current file's tags and mark it modified.
    fn update_tags(&mut self, change: impl FnOnce(&mut Tags)) {
        let Some(entry) = self
            .files
            .selected()
            .and_then(|index| self.entries.get_mut(index))
        else {
            return;
        };
        change(&mut entry.tags);
        entry.modified = true;
        let count = entry.tags.len();
        self.tags.select(match self.tags.selected() {
            _ if count == 0 => None,
            Some(index) => Some(index.min(count - 1)),
            None => Some(0),
        });
    }

    fn save(&mut self) {
        let Some(entry) = self
            .files
            .selected()
            .and_then(|index| self.entries.get_mut(index))
        else {
            return;
        };
        if !entry.modified {
            return;
        }
        match entry.tags.write(&entry.path) {
            Ok(()) => {
                entry.modified = false;
                let name = entry.path.file_name().map_or_else(
                    || entry.label.clone(),
                    |name| name.to_string_lossy().into_owned(),
                );
                self.notify(format!("Saved {name}"), Level::Info);
            }
            Err(error) => self.notify(format!("{error:#}"), Level::Error),
        }
    }

    fn save_all(&mut self) {
        let mut saved = 0;
        let mut failure = None;
        for entry in self.entries.iter_mut().filter(|entry| entry.modified) {
            match entry.tags.write(&entry.path) {
                Ok(()) => {
                    entry.modified = false;
                    saved += 1;
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        match failure {
            Some(error) => self.notify(format!("{error:#}"), Level::Error),
            None => {
                let plural = if saved == 1 { "" } else { "s" };
                self.notify(format!("Saved {saved} file{plural}"), Level::Info);
            }
        }
    }

    fn quit(&mut self) {
        let unsaved = self.entries.iter().filter(|entry| entry.modified).count();
        if unsaved == 0 || self.quit_armed.is_some() {
            self.done = true;
            return;
        }
        self.quit_armed = Some(Instant::now() + QUIT_TIME);
        self.notify(
            format!("{unsaved} unsaved file(s). Press q again to discard."),
            Level::Warning,
        );
    }

    fn notify(&mut self, text: String, level: Level) {
        self.notice = Some(Notice {
            text,
            level,
            until: Instant::now() + NOTICE_TIME,
        });
    }

    fn expire(&mut self) {
        let now = Instant::now();
        if self
            .notice
            .as_ref()
            .is_some_and(|notice| notice.until <= now)
        {
            self.notice = None;
        }
        if self.quit_armed.is_some_and(|until| until <= now) {
            self.quit_armed = None;
        }
    }

    /// Move the focused pane's selection, clamped to its rows.
    fn move_by(&mut self, delta: isize) {
        let (selected, count) = match self.focus {
            Focus::Files => (self.files.selected(), self.entries.len()),
            Focus::Tags => (
                self.tags.selected(),
                self.current().map_or(0, |entry| entry.tags.len()),
            ),
        };
        if count == 0 {
            return;
        }
        let current = selected.unwrap_or(0).cast_signed();
        let target = current
            .saturating_add(delta)
            .clamp(0, (count - 1).cast_signed());
        let target = Some(target.cast_unsigned());
        match self.focus {
            Focus::Files => {
                if target != selected {
                    self.files.select(target);
                    self.reset_tag_selection();
                }
            }
            Focus::Tags => self.tags.select(target),
        }
    }

    fn reset_tag_selection(&mut self) {
        let has_tags = self.current().is_some_and(|entry| !entry.tags.is_empty());
        *self.tags.offset_mut() = 0;
        self.tags.select(has_tags.then_some(0));
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let [files, tags] =
            Layout::horizontal([Constraint::Fill(2), Constraint::Fill(3)]).areas(body);

        frame.render_widget(
            Line::from(vec![
                " audio-tag ".bold().reversed(),
                " ".into(),
                self.root.display().to_string().dim(),
            ]),
            header,
        );
        self.draw_files(frame, files);
        self.draw_tags(frame, tags);
        frame.render_widget(self.footer(), footer);
        if let Some(modal) = &self.modal {
            draw_modal(frame, modal);
        }
    }

    fn draw_files(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| {
                let marker = if entry.modified { "* " } else { "  " };
                ListItem::new(format!("{marker}{}", entry.label))
            })
            .collect();
        let focused = self.focus == Focus::Files;
        let list = List::new(items)
            .block(pane(" Files ", focused))
            .highlight_style(highlight(focused));
        frame.render_stateful_widget(list, area, &mut self.files);
    }

    fn draw_tags(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Tags;
        let Some(entry) = self
            .files
            .selected()
            .and_then(|index| self.entries.get(index))
        else {
            frame.render_widget(pane(" Tags ", focused), area);
            return;
        };
        let name_width = entry
            .tags
            .iter()
            .map(|(name, _)| Line::raw(name).width())
            .max()
            .unwrap_or(0)
            .max(3);
        let rows = entry
            .tags
            .iter()
            .map(|(name, values)| Row::new([name.to_owned(), values.join(SEPARATOR)]));
        let table = Table::new(
            rows,
            [
                Constraint::Length(u16::try_from(name_width).unwrap_or(u16::MAX)),
                Constraint::Fill(1),
            ],
        )
        .header(Row::new(["Tag", "Value"]).bold())
        .column_spacing(2)
        .block(pane(" Tags ", focused))
        .row_highlight_style(highlight(focused));
        frame.render_stateful_widget(table, area, &mut self.tags);
    }

    fn footer(&self) -> Line<'static> {
        if let Some(notice) = &self.notice {
            let color = match notice.level {
                Level::Info => Color::Green,
                Level::Warning => Color::Yellow,
                Level::Error => Color::Red,
            };
            return Line::from(format!(" {}", notice.text)).fg(color);
        }
        let keys: &[(&str, &str)] = match self.modal {
            Some(Modal::Edit { .. }) => &[("enter", "Save"), ("esc", "Cancel")],
            Some(Modal::Add { .. }) => {
                &[("enter", "Next/Add"), ("tab", "Switch"), ("esc", "Cancel")]
            }
            None => &[
                ("q", "Quit"),
                ("a", "Add"),
                ("x", "Delete"),
                ("s", "Save"),
                ("S", "Save All"),
                ("enter", "Edit"),
                ("tab", "Pane"),
            ],
        };
        let mut spans = Vec::new();
        for (key, label) in keys {
            spans.push(Span::from(format!(" {key} ")).bold().reversed());
            spans.push(Span::from(format!(" {label}  ")));
        }
        Line::from(spans)
    }
}

fn draw_modal(frame: &mut Frame, modal: &Modal) {
    let width = (frame.area().width * 4 / 5).min(60);
    let height = match modal {
        Modal::Edit { .. } => 3,
        Modal::Add { .. } => 6,
    };
    let area = frame
        .area()
        .centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, area);
    match modal {
        Modal::Edit { name, field, .. } => field.render(frame, area, &format!("Edit {name}"), true),
        Modal::Add {
            name,
            value,
            on_value,
        } => {
            let [name_area, value_area] =
                Layout::vertical([Constraint::Length(3), Constraint::Length(3)]).areas(area);
            name.render(frame, name_area, "Tag name", !on_value);
            value.render(frame, value_area, "Value", *on_value);
        }
    }
}

fn pane(title: &str, focused: bool) -> Block<'_> {
    let color = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    Block::bordered()
        .title(title)
        .border_style(Style::new().fg(color))
}

fn highlight(focused: bool) -> Style {
    if focused {
        Style::new().reversed()
    } else {
        Style::new().bg(Color::DarkGray)
    }
}
