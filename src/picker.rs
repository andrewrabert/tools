use anyhow::{Context, Result};
use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::{DefaultTerminal, Frame};

/// How the query is matched against the items.
#[derive(Clone, Copy)]
pub enum Matching {
    /// Fuzzy words, where `'`, `^`, `$` and `!` mark a word as exact, a prefix, a suffix or
    /// excluded, best match first.
    Fuzzy,
    /// The whole query as one case-insensitive substring, in the order given.
    Exact,
}

/// What the picker was on when Enter was pressed.
pub struct Picked {
    /// The index of the highlighted item, or `None` when nothing matched.
    pub item: Option<usize>,
    pub query: String,
}

/// Picks one of `items` in a full-screen terminal UI, or `None` when cancelled.
pub fn pick(items: &[Line<'_>], prompt: &str, matching: Matching) -> Result<Option<Picked>> {
    let picker = Picker::new(items, prompt, matching);
    let mut terminal = ratatui::try_init().context("starting the terminal UI")?;
    let result = picker.run(&mut terminal);
    ratatui::restore();
    result
}

/// The text of an item as drawn, which is what the query matches.
struct Haystack {
    item: usize,
    text: String,
}

impl AsRef<str> for Haystack {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

struct Picker<'a> {
    items: &'a [Line<'a>],
    /// One per item, in the same order.
    haystacks: Vec<Haystack>,
    prompt: &'a str,
    matching: Matching,
    matcher: Matcher,
    query: String,
    /// The query as the matcher takes it.
    pattern: Pattern,
    /// The indices of the matching items, in the order listed.
    matches: Vec<usize>,
    selected: usize,
    /// The row of `matches` at the top of the list.
    offset: usize,
}

impl<'a> Picker<'a> {
    fn new(items: &'a [Line<'a>], prompt: &'a str, matching: Matching) -> Self {
        let haystacks = items
            .iter()
            .enumerate()
            .map(|(item, line)| Haystack {
                item,
                text: line
                    .styled_graphemes(Style::default())
                    .map(|grapheme| grapheme.symbol)
                    .collect(),
            })
            .collect();
        let mut picker = Self {
            items,
            haystacks,
            prompt,
            matching,
            matcher: Matcher::new(Config::DEFAULT),
            query: String::new(),
            pattern: Pattern::default(),
            matches: Vec::new(),
            selected: 0,
            offset: 0,
        };
        picker.refilter();
        picker
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<Option<Picked>> {
        loop {
            terminal
                .draw(|frame| self.render(frame))
                .context("drawing the terminal UI")?;
            let Event::Key(key) = event::read().context("reading terminal input")? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c' | 'g') if control => return Ok(None),
                KeyCode::Enter => {
                    return Ok(Some(Picked {
                        item: self.matches.get(self.selected).copied(),
                        query: self.query,
                    }));
                }
                KeyCode::Up => self.select_previous(),
                KeyCode::Char('p' | 'k') if control => self.select_previous(),
                KeyCode::Down => self.select_next(),
                KeyCode::Char('n' | 'j') if control => self.select_next(),
                KeyCode::Backspace => self.backspace(),
                KeyCode::Char('h') if control => self.backspace(),
                KeyCode::Char('u') if control => {
                    self.query.clear();
                    self.refilter();
                }
                KeyCode::Char('w') if control => {
                    let start = self
                        .query
                        .trim_end()
                        .char_indices()
                        .rfind(|(_, c)| c.is_whitespace())
                        .map_or(0, |(index, c)| index + c.len_utf8());
                    self.query.truncate(start);
                    self.refilter();
                }
                KeyCode::Char(c) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                    self.query.push(c);
                    self.refilter();
                }
                _ => {}
            }
        }
    }

    fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.matches.len() {
            self.selected += 1;
        }
    }

    fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.refilter();
        }
    }

    /// Rematches every item against the query and highlights the first match.
    fn refilter(&mut self) {
        match self.matching {
            Matching::Fuzzy => {
                self.pattern
                    .reparse(&self.query, CaseMatching::Smart, Normalization::Smart);
            }
            Matching::Exact => {
                self.pattern.atoms.clear();
                if !self.query.is_empty() {
                    self.pattern.atoms.push(Atom::new(
                        &self.query,
                        CaseMatching::Ignore,
                        Normalization::Smart,
                        AtomKind::Substring,
                        false,
                    ));
                }
            }
        }
        self.matches = self
            .pattern
            .match_list(&self.haystacks, &mut self.matcher)
            .into_iter()
            .map(|(haystack, _)| haystack.item)
            .collect();
        if matches!(self.matching, Matching::Exact) {
            self.matches.sort_unstable();
        }
        self.selected = 0;
        self.offset = 0;
    }

    fn render(&mut self, frame: &mut Frame) {
        let [input, list] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(frame.area());

        let line = Line::from(vec![
            Span::styled(self.prompt, Style::new().fg(Color::Cyan)),
            Span::raw(self.query.as_str()),
        ]);
        let width = u16::try_from(line.width()).unwrap_or(u16::MAX);
        frame.render_widget(line, input);
        frame.set_cursor_position((input.x + width.min(input.width.saturating_sub(1)), input.y));

        let height = usize::from(list.height).max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
        let end = self.matches.len().min(self.offset + height);

        let mut buffer = Vec::new();
        let mut indices = Vec::new();
        let selected = self.selected - self.offset;
        let rows = self.matches[self.offset..end]
            .iter()
            .enumerate()
            .map(|(row, &item)| {
                indices.clear();
                self.pattern.indices(
                    Utf32Str::new(&self.haystacks[item].text, &mut buffer),
                    &mut self.matcher,
                    &mut indices,
                );
                indices.sort_unstable();
                indices.dedup();
                let line = highlight(&self.items[item], &indices, row == selected);
                if row == selected {
                    ListItem::new(line).style(
                        Style::new()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ListItem::new(line)
                }
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default().with_selected((!rows.is_empty()).then_some(selected));
        frame.render_stateful_widget(List::new(rows).highlight_symbol("> "), list, &mut state);
    }
}

/// `line` with the graphemes at `indices` colored as matched. On the `selected` row, the
/// graphemes with no color of their own are drawn in the terminal's background color.
fn highlight(line: &Line<'_>, indices: &[u32], selected: bool) -> Line<'static> {
    let spans = line
        .styled_graphemes(Style::default())
        .zip(0u32..)
        .map(|(grapheme, index)| {
            let style = if indices.binary_search(&index).is_ok() {
                grapheme.style.fg(Color::Green)
            } else {
                grapheme.style
            };
            let style = if selected && style.fg.is_none() {
                // Reversed, the foreground is drawn as the background and the reverse.
                style
                    .fg(Color::DarkGray)
                    .bg(Color::Reset)
                    .add_modifier(Modifier::REVERSED)
            } else {
                style
            };
            Span::styled(grapheme.symbol.to_owned(), style)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}
