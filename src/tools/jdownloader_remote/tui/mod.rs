//! An interactive terminal UI over the same API the subcommands use.

mod form;
mod view;

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs, Wrap,
};
use ratatui::{DefaultTerminal, Frame};
use serde_json::Value as Json;

use crate::tools::jdownloader_remote::api::Api;
use crate::tools::jdownloader_remote::cli::{CleanupActions, Sort};
use crate::tools::jdownloader_remote::model::{
    AccountId, AddLinks, ArchiveId, CaptchaId, CleanupMode, DupesAction, LinkId, LinkVariant,
    PackageId, Priority, VariantId,
};
use crate::tools::jdownloader_remote::tui::form::{Field, Form, FormEvent};
use crate::tools::jdownloader_remote::tui::view::{
    Columns, Data, Fold, Loaded, Node, Tab, Tone, TreeFilter,
};
use crate::tools::jdownloader_remote::{add_container, cleanup_actions, sort_links};
use crate::urls::{Scheme, unique_urls};

const TICK: Duration = Duration::from_millis(100);
const REFRESH: Duration = Duration::from_secs(2);
const NOTICE: Duration = Duration::from_secs(5);
const PAGE: isize = 10;
/// Separates the container paths typed into one field.
const PATH_SEPARATOR: char = ';';
/// The dupes choice that leaves JDownloader's setting as it is.
const DUPES_UNCHANGED: &str = "unchanged";

const HELP: &str = "\
Global
  1-8 Tab S-Tab   switch tab             r F5     refresh
  j k ↑ ↓ PgUp PgDn Home End  move       Enter    show item as JSON
  Space           mark row               *        mark all shown rows
  /               filter rows            Esc      clear marks, then filter
  s S             start / stop downloads z Z      pause / unpause
  a               add links              A        add container files
  C               clean up download list ?        help
  q Ctrl-C        quit

Actions apply to the marked rows, or the row under the cursor.

Downloads (packages, each holding its links)
  → l    open package  ← h  close package, or go to a link's package
  ]      open all      [    close all
  d Del  remove        x  reset         u  resume
  e E    enable/disable n  rename        f  set download folder
  +      priority      X  extract archives
  o      sort links    t  running only
  w      all columns / state, name and size

Grabber
  y  confirm and start   Y  confirm without starting
  v  choose variant      D  clear the grabber

Captchas
  y  solve   x  skip   w  save image

Accounts
  n  add   d Del  remove   e E  enable/disable   u  refresh status

Archives    p  set password
Config      e  edit value   v  read current value
Plugins     p  show plugin settings";

type Job = Box<dyn FnOnce(&Api) -> Outcome + Send>;
/// A change to JDownloader that reports what it did.
type Action = Box<dyn FnOnce(&Api) -> Result<String> + Send>;

enum Outcome {
    Loaded(Tab, Result<Loaded>),
    Done(Result<String>),
    Variants(LinkId, Result<Vec<LinkVariant>>),
    Text(String, Result<String>),
}

pub fn run(api: Api) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("the tui needs a terminal");
    }
    let (jobs, job_queue) = mpsc::channel::<Job>();
    let (outcome_sender, outcomes) = mpsc::channel();
    thread::spawn(move || {
        for job in job_queue {
            if outcome_sender.send(job(&api)).is_err() {
                break;
            }
        }
    });
    let mut terminal = ratatui::try_init().context("starting the terminal UI")?;
    let result = execute!(io::stdout(), EnableBracketedPaste)
        .context("enabling bracketed paste")
        .and_then(|()| App::new(jobs, outcomes).run(&mut terminal));
    // The terminal is restored even when bracketed paste cannot be turned off.
    let disabled = execute!(io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result.and(disabled.context("disabling bracketed paste"))
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

#[derive(Default)]
struct View {
    state: TableState,
    marks: BTreeSet<String>,
    filter: String,
    /// The keys of the packages the user opened in the download tree.
    expanded: BTreeSet<String>,
}

struct Notice {
    text: String,
    error: bool,
    at: Instant,
}

enum Pick {
    Priority {
        links: Vec<LinkId>,
        packages: Vec<PackageId>,
    },
    Variant {
        link: LinkId,
        variants: Vec<VariantId>,
    },
}

enum Submit {
    AddLinks,
    AddContainers,
    Cleanup {
        links: Vec<LinkId>,
        packages: Vec<PackageId>,
    },
    RenameLink(LinkId),
    RenamePackage(PackageId),
    SetFolder(Vec<PackageId>),
    Solve(CaptchaId),
    SaveCaptcha(CaptchaId),
    AddAccount,
    ArchivePassword(ArchiveId),
    ConfigSet {
        interface: String,
        key: String,
    },
}

enum Modal {
    Form(Form, Submit),
    Confirm(String, Action),
    Pick {
        title: String,
        options: Vec<String>,
        state: ListState,
        pick: Pick,
    },
    Text {
        title: String,
        body: String,
        scroll: u16,
    },
}

/// What a bulk action on the download list or grabber applies to.
struct Selection {
    links: Vec<LinkId>,
    packages: Vec<PackageId>,
}

impl Selection {
    fn describe(&self) -> String {
        let count = |n: usize, noun: &str| format!("{n} {noun}{}", if n == 1 { "" } else { "s" });
        match (self.links.len(), self.packages.len()) {
            (links, 0) => count(links, "link"),
            (0, packages) => count(packages, "package"),
            (links, packages) => format!(
                "{} and {}",
                count(links, "link"),
                count(packages, "package")
            ),
        }
    }
}

struct App {
    jobs: Sender<Job>,
    outcomes: Receiver<Outcome>,
    data: Data,
    tab: Tab,
    views: [View; Tab::ALL.len()],
    loaded: [bool; Tab::ALL.len()],
    loading: [bool; Tab::ALL.len()],
    sort: Sort,
    running_only: bool,
    columns: Columns,
    filtering: bool,
    info_scroll: u16,
    modal: Option<Modal>,
    notice: Option<Notice>,
    pending: usize,
    refreshed: Instant,
    done: bool,
}

impl App {
    fn new(jobs: Sender<Job>, outcomes: Receiver<Outcome>) -> Self {
        Self {
            jobs,
            outcomes,
            data: Data::default(),
            tab: Tab::Downloads,
            views: Default::default(),
            loaded: [false; Tab::ALL.len()],
            loading: [false; Tab::ALL.len()],
            sort: Sort::Name,
            running_only: false,
            columns: Columns::Compact,
            filtering: false,
            info_scroll: 0,
            modal: None,
            notice: None,
            pending: 0,
            refreshed: Instant::now(),
            done: false,
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        for tab in Tab::ALL.into_iter().filter(|tab| tab.live()) {
            self.load(tab);
        }
        while !self.done {
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(TICK)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key),
                    Event::Paste(text) => self.paste(&text),
                    _ => {}
                }
            }
            loop {
                match self.outcomes.try_recv() {
                    Ok(outcome) => self.receive(outcome),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => bail!("the api worker stopped"),
                }
            }
            self.tick();
        }
        Ok(())
    }

    fn submit(&mut self, job: Job) {
        if self.jobs.send(job).is_err() {
            self.error("the api worker stopped");
        } else {
            self.pending += 1;
        }
    }

    fn load(&mut self, tab: Tab) {
        if self.loading[tab.index()] {
            return;
        }
        self.loading[tab.index()] = true;
        if tab == self.tab {
            self.refreshed = Instant::now();
        }
        self.submit(Box::new(move |api| {
            Outcome::Loaded(tab, Loaded::fetch(api, tab))
        }));
    }

    fn perform(&mut self, action: Action) {
        self.submit(Box::new(move |api| Outcome::Done(action(api))));
    }

    /// Runs a change that reports `message` when it succeeds.
    fn act(
        &mut self,
        message: impl Into<String>,
        change: impl FnOnce(&Api) -> Result<()> + Send + 'static,
    ) {
        let message = message.into();
        self.perform(Box::new(move |api| change(api).map(|()| message)));
    }

    fn receive(&mut self, outcome: Outcome) {
        self.pending = self.pending.saturating_sub(1);
        match outcome {
            Outcome::Loaded(tab, result) => {
                self.loading[tab.index()] = false;
                match result {
                    Ok(loaded) => {
                        self.data.store(loaded);
                        if tab == Tab::Downloads {
                            sort_links(&mut self.data.links, self.sort);
                        }
                        self.loaded[tab.index()] = true;
                        self.clamp(tab);
                    }
                    Err(error) => self.error(format!("loading {}: {error:#}", tab.title())),
                }
            }
            Outcome::Done(Ok(message)) => {
                self.info(message);
                self.loaded = [false; Tab::ALL.len()];
                self.load(self.tab);
            }
            Outcome::Done(Err(error)) => self.error(format!("{error:#}")),
            Outcome::Variants(link, Ok(variants)) => {
                if variants.is_empty() {
                    self.info("the link has no variants");
                    return;
                }
                let (ids, options) = variants
                    .into_iter()
                    .filter_map(|variant| {
                        let id = variant.id?;
                        let name = variant.name.unwrap_or_else(|| id.to_string());
                        Some((id, name))
                    })
                    .unzip();
                self.modal = Some(Modal::Pick {
                    title: "Variant".to_owned(),
                    options,
                    state: ListState::default().with_selected(Some(0)),
                    pick: Pick::Variant {
                        link,
                        variants: ids,
                    },
                });
            }
            Outcome::Text(title, Ok(body)) => {
                self.modal = Some(Modal::Text {
                    title,
                    body,
                    scroll: 0,
                });
            }
            Outcome::Variants(_, Err(error)) | Outcome::Text(_, Err(error)) => {
                self.error(format!("{error:#}"));
            }
        }
    }

    fn tick(&mut self) {
        if self
            .notice
            .as_ref()
            .is_some_and(|notice| !notice.error && notice.at.elapsed() >= NOTICE)
        {
            self.notice = None;
        }
        if self.tab.live() && self.refreshed.elapsed() >= REFRESH {
            self.load(self.tab);
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
            at: Instant::now(),
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: true,
            at: Instant::now(),
        });
    }

    fn view(&self) -> &View {
        &self.views[self.tab.index()]
    }

    fn view_mut(&mut self) -> &mut View {
        &mut self.views[self.tab.index()]
    }

    /// The rows the tab shows, in order.
    fn visible(&self, tab: Tab) -> Vec<Node> {
        let view = &self.views[tab.index()];
        let filter = view.filter.to_lowercase();
        if tab == Tab::Downloads {
            let filter = TreeFilter {
                text: &filter,
                running_only: self.running_only,
            };
            return self.data.tree(&view.expanded, &filter);
        }
        self.data
            .rows(tab)
            .into_iter()
            .filter(|&node| self.data.matches(node, &filter))
            .collect()
    }

    fn clamp(&mut self, tab: Tab) {
        let len = self.visible(tab).len();
        let state = &mut self.views[tab.index()].state;
        match (len, state.selected()) {
            (0, _) => state.select(None),
            (_, None) => state.select(Some(0)),
            (len, Some(selected)) => state.select(Some(selected.min(len - 1))),
        }
    }

    /// The row under the cursor.
    fn cursor(&self) -> Option<Node> {
        let selected = self.view().state.selected()?;
        self.visible(self.tab).get(selected).copied()
    }

    /// The marked rows that are shown, or else the row under the cursor.
    fn targets(&self) -> Vec<Node> {
        let marks = &self.view().marks;
        let marked: Vec<Node> = self
            .visible(self.tab)
            .into_iter()
            .filter(|&node| marks.contains(&self.data.key(node)))
            .collect();
        if marked.is_empty() {
            self.cursor().into_iter().collect()
        } else {
            marked
        }
    }

    fn selection(&self) -> Option<Selection> {
        let mut selection = Selection {
            links: vec![],
            packages: vec![],
        };
        for node in self.targets() {
            match node {
                Node::Package(i, _) => selection.packages.push(self.data.packages[i].uuid),
                Node::Link(i) => selection.links.push(self.data.links[i].uuid),
                Node::Grabber(i) => selection.links.push(self.data.grabber[i].uuid),
                Node::Captcha(_)
                | Node::Account(_)
                | Node::Archive(_)
                | Node::Config(_)
                | Node::Plugin(_) => {}
            }
        }
        (!selection.links.is_empty() || !selection.packages.is_empty()).then_some(selection)
    }

    /// Opens or closes the package under the cursor; closing on a link closes its package.
    fn fold(&mut self, fold: Fold) {
        let package = match self.cursor() {
            Some(Node::Package(i, _)) => i,
            Some(Node::Link(i)) if fold == Fold::Closed => {
                let parent = i64::from(self.data.links[i].package_uuid);
                let Some(package) = self
                    .data
                    .packages
                    .iter()
                    .position(|package| i64::from(package.uuid) == parent)
                else {
                    return;
                };
                package
            }
            _ => return,
        };
        let key = self.data.key(Node::Package(package, fold));
        let expanded = &mut self.view_mut().expanded;
        match fold {
            Fold::Open => expanded.insert(key),
            Fold::Closed => expanded.remove(&key),
        };
        let row = self
            .visible(self.tab)
            .iter()
            .position(|node| matches!(node, Node::Package(i, _) if *i == package));
        if let Some(row) = row {
            self.view_mut().state.select(Some(row));
        }
        self.clamp(self.tab);
    }

    fn fold_all(&mut self, fold: Fold) {
        let keys: BTreeSet<String> = match fold {
            Fold::Open => (0..self.data.packages.len())
                .map(|i| self.data.key(Node::Package(i, fold)))
                .collect(),
            Fold::Closed => BTreeSet::new(),
        };
        self.view_mut().expanded = keys;
        self.clamp(self.tab);
    }

    fn switch(&mut self, tab: Tab) {
        self.tab = tab;
        self.filtering = false;
        if !self.loaded[tab.index()] {
            self.load(tab);
        }
        self.refreshed = Instant::now();
        self.clamp(tab);
    }

    fn step_tab(&mut self, by: isize) {
        let len = Tab::ALL.len() as isize;
        let index = (self.tab.index() as isize + by).rem_euclid(len) as usize;
        self.switch(Tab::ALL[index]);
    }

    fn move_by(&mut self, by: isize) {
        if self.tab == Tab::Info {
            self.info_scroll = scroll(self.info_scroll, by);
            return;
        }
        let len = self.visible(self.tab).len();
        if len == 0 {
            return;
        }
        let state = &mut self.view_mut().state;
        let current = state.selected().unwrap_or(0) as isize;
        let next = current.saturating_add(by).clamp(0, len as isize - 1);
        state.select(Some(next as usize));
    }

    fn paste(&mut self, text: &str) {
        match &mut self.modal {
            Some(Modal::Form(form, _)) => form.paste(text),
            Some(_) => {}
            None if self.filtering => self.view_mut().filter.push_str(text.trim()),
            None => self.open_add_links(text),
        }
    }

    fn key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.done = true;
            return;
        }
        if let Some(modal) = self.modal.take() {
            self.modal_key(modal, key);
            return;
        }
        if self.filtering {
            self.filter_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.done = true,
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Text {
                    title: "Keys".to_owned(),
                    body: HELP.to_owned(),
                    scroll: 0,
                });
            }
            KeyCode::Tab => self.step_tab(1),
            KeyCode::BackTab => self.step_tab(-1),
            KeyCode::Char(c @ '1'..='9') => {
                let index = usize::from(c as u8 - b'1');
                if let Some(&tab) = Tab::ALL.get(index) {
                    self.switch(tab);
                }
            }
            KeyCode::Char('r') | KeyCode::F(5) => self.load(self.tab),
            KeyCode::Char('s') => self.act("started downloading", Api::start),
            KeyCode::Char('S') => self.act("stopped downloading", Api::stop),
            KeyCode::Char('z') => self.act("paused downloading", |api| api.pause(true)),
            KeyCode::Char('Z') => self.act("unpaused downloading", |api| api.pause(false)),
            KeyCode::Char('a') => self.open_add_links(""),
            KeyCode::Char('A') => self.open_add_containers(),
            KeyCode::Char('C') => self.open_cleanup(),
            KeyCode::Char('/') if self.tab != Tab::Info => self.filtering = true,
            KeyCode::Esc => {
                let view = self.view_mut();
                if view.marks.is_empty() {
                    view.filter.clear();
                } else {
                    view.marks.clear();
                }
                self.clamp(self.tab);
            }
            KeyCode::Char(' ') if self.tab.markable() => {
                if let Some(node) = self.cursor() {
                    let key = self.data.key(node);
                    let marks = &mut self.view_mut().marks;
                    if !marks.remove(&key) {
                        marks.insert(key);
                    }
                    self.move_by(1);
                }
            }
            KeyCode::Char('*') if self.tab.markable() => {
                let keys: BTreeSet<String> = self
                    .visible(self.tab)
                    .into_iter()
                    .map(|node| self.data.key(node))
                    .collect();
                let marks = &mut self.view_mut().marks;
                if keys.is_subset(marks) {
                    marks.retain(|key| !keys.contains(key));
                } else {
                    marks.extend(keys);
                }
            }
            KeyCode::Enter => {
                if let Some(node) = self.cursor() {
                    self.modal = Some(Modal::Text {
                        title: self.tab.title().to_owned(),
                        body: self.data.details(node),
                        scroll: 0,
                    });
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-PAGE),
            KeyCode::PageDown => self.move_by(PAGE),
            KeyCode::Home => self.move_by(isize::MIN),
            KeyCode::End => self.move_by(isize::MAX),
            code => self.tab_key(code),
        }
    }

    fn filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.filtering = false,
            KeyCode::Esc => {
                self.filtering = false;
                self.view_mut().filter.clear();
            }
            KeyCode::Backspace => {
                self.view_mut().filter.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.view_mut().filter.clear();
            }
            KeyCode::Char(c) => self.view_mut().filter.push(c),
            _ => {}
        }
        self.clamp(self.tab);
    }

    fn tab_key(&mut self, code: KeyCode) {
        use KeyCode::{Char, Delete, Left, Right};
        match (self.tab, code) {
            (Tab::Downloads, Char('d') | Delete) => {
                self.bulk("removed", true, |api, s| api.remove(&s.links, &s.packages));
            }
            (Tab::Downloads, Char('x')) => {
                self.bulk("reset", true, |api, s| api.reset(&s.links, &s.packages));
            }
            (Tab::Downloads, Char('u')) => {
                self.bulk("resumed", false, |api, s| api.resume(&s.links, &s.packages));
            }
            (Tab::Downloads, Char('e')) => {
                self.bulk("enabled", false, |api, s| {
                    api.set_enabled(true, &s.links, &s.packages)
                });
            }
            (Tab::Downloads, Char('E')) => {
                self.bulk("disabled", false, |api, s| {
                    api.set_enabled(false, &s.links, &s.packages)
                });
            }
            (Tab::Downloads, Char('X')) => {
                self.bulk("started extracting", false, |api, s| {
                    api.start_extraction(&s.links, &s.packages)
                });
            }
            (Tab::Downloads, Char('n')) => self.open_rename(),
            (Tab::Downloads, Char('f')) => self.open_set_folder(),
            (Tab::Downloads, Char('+')) => self.open_priority(),
            (Tab::Downloads, Right | Char('l')) => self.fold(Fold::Open),
            (Tab::Downloads, Left | Char('h')) => self.fold(Fold::Closed),
            (Tab::Downloads, Char(']')) => self.fold_all(Fold::Open),
            (Tab::Downloads, Char('[')) => self.fold_all(Fold::Closed),
            (Tab::Downloads, Char('o')) => {
                let sorts = Sort::value_variants();
                let index = sorts
                    .iter()
                    .position(|sort| value_name(sort) == value_name(&self.sort))
                    .map_or(0, |index| (index + 1) % sorts.len());
                self.sort = sorts[index];
                sort_links(&mut self.data.links, self.sort);
                self.info(format!("sorted by {}", value_name(&self.sort)));
            }
            (Tab::Downloads, Char('w')) => self.columns = self.columns.toggled(),
            (Tab::Downloads, Char('t')) => {
                self.running_only = !self.running_only;
                self.clamp(self.tab);
            }
            (Tab::Grabber, Char('y')) => self.confirm_grabber(true),
            (Tab::Grabber, Char('Y')) => self.confirm_grabber(false),
            (Tab::Grabber, Char('D')) => {
                self.modal = Some(Modal::Confirm(
                    "Clear the link grabber?".to_owned(),
                    Box::new(|api| {
                        api.clear_grabber()
                            .map(|()| "cleared the grabber".to_owned())
                    }),
                ));
            }
            (Tab::Grabber, Char('v')) => {
                if let Some(Node::Grabber(i)) = self.cursor() {
                    let link = self.data.grabber[i].uuid;
                    self.submit(Box::new(move |api| {
                        Outcome::Variants(link, api.variants(link))
                    }));
                }
            }
            (Tab::Captchas, Char('y')) => {
                if let Some(Node::Captcha(i)) = self.cursor() {
                    let captcha = &self.data.captchas[i];
                    let title = format!(
                        "Solve captcha {} ({})",
                        captcha.id,
                        captcha.hoster.as_deref().unwrap_or("unknown hoster")
                    );
                    self.open_form(
                        title,
                        vec![Field::text("Answer", "")],
                        Submit::Solve(captcha.id),
                    );
                }
            }
            (Tab::Captchas, Char('x')) => {
                if let Some(Node::Captcha(i)) = self.cursor() {
                    let id = self.data.captchas[i].id;
                    self.act(format!("skipped captcha {id}"), move |api| {
                        api.skip_captcha(id)
                    });
                }
            }
            (Tab::Captchas, Char('w')) => {
                if let Some(Node::Captcha(i)) = self.cursor() {
                    let id = self.data.captchas[i].id;
                    self.open_form(
                        format!("Save captcha {id} (empty: captcha-{id}.<type>)"),
                        vec![Field::text("File", "")],
                        Submit::SaveCaptcha(id),
                    );
                }
            }
            (Tab::Accounts, Char('n')) => self.open_form(
                "Add hoster account",
                vec![
                    Field::text("Hoster", ""),
                    Field::text("Username", ""),
                    Field::secret("Password"),
                ],
                Submit::AddAccount,
            ),
            (Tab::Accounts, Char('d') | Delete) => {
                self.accounts("removed", true, |api, ids| api.remove_accounts(&ids));
            }
            (Tab::Accounts, Char('e')) => {
                self.accounts("enabled", false, |api, ids| api.enable_accounts(&ids));
            }
            (Tab::Accounts, Char('E')) => {
                self.accounts("disabled", false, |api, ids| api.disable_accounts(&ids));
            }
            (Tab::Accounts, Char('u')) => {
                self.accounts("refreshed", false, |api, ids| api.refresh_accounts(&ids));
            }
            (Tab::Archives, Char('p')) => {
                if let Some(Node::Archive(i)) = self.cursor() {
                    let archive = &self.data.archives[i];
                    let title = format!(
                        "Password for {}",
                        archive
                            .archive_name
                            .as_deref()
                            .unwrap_or(archive.archive_id.as_str())
                    );
                    self.open_form(
                        title,
                        vec![Field::text("Password", "")],
                        Submit::ArchivePassword(archive.archive_id.clone()),
                    );
                }
            }
            (Tab::Config, Char('e')) => {
                if let Some(Node::Config(i)) = self.cursor() {
                    let entry = &self.data.config[i];
                    self.open_form(
                        format!("{} {}", entry.interface_name, entry.key),
                        vec![Field::text("JSON", entry.value.to_string())],
                        Submit::ConfigSet {
                            interface: entry.interface_name.clone(),
                            key: entry.key.clone(),
                        },
                    );
                }
            }
            (Tab::Config, Char('v')) => {
                if let Some(Node::Config(i)) = self.cursor() {
                    let entry = &self.data.config[i];
                    let (interface, key) = (entry.interface_name.clone(), entry.key.clone());
                    let title = format!("{interface} {key}");
                    self.submit(Box::new(move |api| {
                        let value = api.config_get(&interface, &key).and_then(|value| {
                            serde_json::to_string_pretty(&value).context("formatting the value")
                        });
                        Outcome::Text(title, value)
                    }));
                }
            }
            (Tab::Plugins, Char('p')) => {
                if let Some(Node::Plugin(i)) = self.cursor() {
                    let hoster = self.data.plugins[i].display_name.clone();
                    let title = format!("{hoster} settings");
                    self.submit(Box::new(move |api| {
                        Outcome::Text(title, api.plugin_settings(&hoster).map(plugin_settings))
                    }));
                }
            }
            _ => {}
        }
    }

    /// Runs a change on the selected links or packages, asking first when `confirm` is set.
    fn bulk(
        &mut self,
        verb: &'static str,
        confirm: bool,
        change: impl FnOnce(&Api, &Selection) -> Result<()> + Send + 'static,
    ) {
        let Some(selection) = self.selection() else {
            self.error("nothing selected");
            return;
        };
        let description = selection.describe();
        let message = format!("{verb} {description}");
        let action: Action = Box::new(move |api| change(api, &selection).map(|()| message));
        if confirm {
            self.modal = Some(Modal::Confirm(
                format!("{} {description}?", capitalize(verb)),
                action,
            ));
        } else {
            self.perform(action);
        }
    }

    fn accounts(
        &mut self,
        verb: &'static str,
        confirm: bool,
        change: impl FnOnce(&Api, Vec<AccountId>) -> Result<()> + Send + 'static,
    ) {
        let ids: Vec<AccountId> = self
            .targets()
            .into_iter()
            .filter_map(|node| match node {
                Node::Account(i) => Some(self.data.accounts[i].uuid),
                _ => None,
            })
            .collect();
        if ids.is_empty() {
            self.error("nothing selected");
            return;
        }
        let count = ids.len();
        let noun = if count == 1 { "account" } else { "accounts" };
        let message = format!("{verb} {count} {noun}");
        let action: Action = Box::new(move |api| change(api, ids).map(|()| message));
        if confirm {
            self.modal = Some(Modal::Confirm(
                format!("{} {count} {noun}?", capitalize(verb)),
                action,
            ));
        } else {
            self.perform(action);
        }
    }

    fn confirm_grabber(&mut self, start: bool) {
        let Some(selection) = self.selection() else {
            self.error("nothing selected");
            return;
        };
        let message = format!("moved {} to the download list", selection.describe());
        self.act(message, move |api| {
            api.confirm(&selection.links, &selection.packages)?;
            if start {
                api.start()?;
            }
            Ok(())
        });
    }

    fn open_form(&mut self, title: impl Into<String>, fields: Vec<Field>, submit: Submit) {
        self.modal = Some(Modal::Form(Form::new(title, fields), submit));
    }

    fn open_add_links(&mut self, urls: &str) {
        self.open_form(
            "Add links",
            vec![
                Field::text("URLs", urls.replace(['\n', '\r'], " ")),
                Field::text("Package", ""),
                Field::text("Folder", ""),
                Field::text("Extract password", ""),
                Field::text("Download password", ""),
                Field::toggle("Start", true),
                dupes_field(),
            ],
            Submit::AddLinks,
        );
    }

    fn open_add_containers(&mut self) {
        self.open_form(
            format!("Add container files (separate paths with {PATH_SEPARATOR})"),
            vec![
                Field::text("Files", ""),
                Field::toggle("Delete after adding", false),
                dupes_field(),
            ],
            Submit::AddContainers,
        );
    }

    fn open_cleanup(&mut self) {
        let modes: Vec<String> = CleanupMode::value_variants()
            .iter()
            .map(value_name)
            .collect();
        let links_only = value_name(&CleanupMode::LinksOnly);
        let mode = modes
            .iter()
            .position(|mode| *mode == links_only)
            .unwrap_or(0);
        let mut fields = vec![
            Field::toggle("All", false),
            Field::toggle("Disabled", false),
            Field::toggle("Failed", false),
            Field::toggle("Finished", true),
            Field::toggle("Offline", false),
            Field::toggle("Dupe", false),
            Field::choice("Mode", modes, mode),
        ];
        let selection = match self.tab {
            Tab::Downloads => self.selection(),
            _ => None,
        };
        let (links, packages) = match selection {
            Some(selection) => {
                fields.push(Field::choice(
                    "Scope",
                    vec!["entire list".to_owned(), selection.describe()],
                    0,
                ));
                (selection.links, selection.packages)
            }
            None => (vec![], vec![]),
        };
        self.open_form(
            "Clean up the download list",
            fields,
            Submit::Cleanup { links, packages },
        );
    }

    fn open_rename(&mut self) {
        let (name, submit) = match self.cursor() {
            Some(Node::Link(i)) => {
                let link = &self.data.links[i];
                (link.name.clone(), Submit::RenameLink(link.uuid))
            }
            Some(Node::Package(i, _)) => {
                let package = &self.data.packages[i];
                (package.name.clone(), Submit::RenamePackage(package.uuid))
            }
            _ => return,
        };
        self.open_form("Rename", vec![Field::text("Name", name)], submit);
    }

    fn open_set_folder(&mut self) {
        let mut packages: Vec<PackageId> = self
            .targets()
            .into_iter()
            .filter_map(|node| match node {
                Node::Link(i) => Some(self.data.links[i].package_uuid),
                Node::Package(i, _) => Some(self.data.packages[i].uuid),
                _ => None,
            })
            .collect();
        packages.sort_by_key(|&id| i64::from(id));
        packages.dedup_by_key(|id| i64::from(*id));
        let Some(&first) = packages.first() else {
            self.error("nothing selected");
            return;
        };
        let folder = self
            .data
            .packages
            .iter()
            .find(|package| i64::from(package.uuid) == i64::from(first))
            .map(|package| package.save_to.clone())
            .unwrap_or_default();
        let count = packages.len();
        let noun = if count == 1 { "package" } else { "packages" };
        self.open_form(
            format!("Download folder of {count} {noun}"),
            vec![Field::text("Folder", folder)],
            Submit::SetFolder(packages),
        );
    }

    fn open_priority(&mut self) {
        let Some(Selection { links, packages }) = self.selection() else {
            self.error("nothing selected");
            return;
        };
        self.modal = Some(Modal::Pick {
            title: "Priority".to_owned(),
            options: Priority::value_variants().iter().map(value_name).collect(),
            state: ListState::default().with_selected(Some(0)),
            pick: Pick::Priority { links, packages },
        });
    }

    fn modal_key(&mut self, modal: Modal, key: KeyEvent) {
        match modal {
            Modal::Form(mut form, submit) => match form.key(key) {
                FormEvent::Pending => self.modal = Some(Modal::Form(form, submit)),
                FormEvent::Cancel => {}
                FormEvent::Submit => match action(&form, &submit) {
                    Ok(action) => self.perform(action),
                    Err(error) => {
                        self.error(format!("{error:#}"));
                        self.modal = Some(Modal::Form(form, submit));
                    }
                },
            },
            Modal::Confirm(prompt, action) => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.perform(action),
                KeyCode::Char('n') | KeyCode::Esc => {}
                _ => self.modal = Some(Modal::Confirm(prompt, action)),
            },
            Modal::Pick {
                title,
                options,
                mut state,
                pick,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if let Some(index) = state.selected() {
                        self.picked(pick, index);
                    }
                }
                code => {
                    match code {
                        KeyCode::Up | KeyCode::Char('k') => state.select_previous(),
                        KeyCode::Down | KeyCode::Char('j') => state.select_next(),
                        _ => {}
                    }
                    self.modal = Some(Modal::Pick {
                        title,
                        options,
                        state,
                        pick,
                    });
                }
            },
            Modal::Text {
                title,
                body,
                scroll: offset,
            } => {
                let offset = match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => return,
                    KeyCode::Up | KeyCode::Char('k') => scroll(offset, -1),
                    KeyCode::Down | KeyCode::Char('j') => scroll(offset, 1),
                    KeyCode::PageUp => scroll(offset, -PAGE),
                    KeyCode::PageDown => scroll(offset, PAGE),
                    KeyCode::Home => 0,
                    _ => offset,
                };
                self.modal = Some(Modal::Text {
                    title,
                    body,
                    scroll: offset,
                });
            }
        }
    }

    fn picked(&mut self, pick: Pick, index: usize) {
        match pick {
            Pick::Priority { links, packages } => {
                let Some(&priority) = Priority::value_variants().get(index) else {
                    return;
                };
                let message = format!("set priority {}", value_name(&priority));
                self.act(message, move |api| {
                    api.set_priority(priority, &links, &packages)
                });
            }
            Pick::Variant { link, variants } => {
                let Some(variant) = variants.into_iter().nth(index) else {
                    return;
                };
                let message = format!("set variant {variant}");
                self.act(message, move |api| api.set_variant(link, &variant));
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [tabs, main, status, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        self.draw_tabs(frame, tabs);
        if self.tab == Tab::Info {
            self.draw_info(frame, main);
        } else {
            self.draw_table(frame, main);
        }
        self.draw_status(frame, status);
        self.draw_footer(frame, footer);
        if let Some(modal) = &self.modal {
            draw_modal(frame, modal);
        }
    }

    fn draw_tabs(&self, frame: &mut Frame, area: Rect) {
        let titles: Vec<Line> = Tab::ALL
            .iter()
            .enumerate()
            .map(|(index, &tab)| {
                let count = match tab {
                    Tab::Info | Tab::Config | Tab::Plugins => String::new(),
                    _ if self.loaded[tab.index()] || self.data.len(tab) > 0 => {
                        format!(" ({})", self.data.len(tab))
                    }
                    _ => String::new(),
                };
                let line = Line::from(format!("{} {}{count}", index + 1, tab.title()));
                if tab == Tab::Captchas && !self.data.captchas.is_empty() {
                    line.fg(Color::Yellow)
                } else {
                    line
                }
            })
            .collect();
        let tabs = Tabs::new(titles)
            .select(self.tab.index())
            .highlight_style(Style::new().bold().reversed())
            .divider("│");
        frame.render_widget(tabs, area);
    }

    fn draw_table(&mut self, frame: &mut Frame, area: Rect) {
        let tab = self.tab;
        let visible = self.visible(tab);
        let markable = tab.markable();
        let view = &self.views[tab.index()];
        let mut widths = tab.widths(self.columns);
        let mut headers: Vec<&str> = tab.headers(self.columns).to_vec();
        if markable {
            widths.insert(0, Constraint::Length(1));
            headers.insert(0, "");
        }
        let rows: Vec<Row> = visible
            .iter()
            .map(|&node| {
                let mut cells = self.data.cells(node, self.columns);
                if markable {
                    let marked = view.marks.contains(&self.data.key(node));
                    cells.insert(0, if marked { "●" } else { " " }.to_owned());
                }
                let mut style = match self.data.tone(node) {
                    Tone::Normal => Style::new(),
                    Tone::Disabled => Style::new().dim(),
                    Tone::Active => Style::new().fg(Color::Cyan),
                    Tone::Finished => Style::new().fg(Color::Green),
                    Tone::Failed => Style::new().fg(Color::Red),
                };
                if let Node::Package(..) = node {
                    style = style.bold();
                }
                Row::new(cells).style(style)
            })
            .collect();

        let mut title = vec![format!(" {} ", tab.title())];
        if tab == Tab::Downloads {
            title.push(format!("{} packages ", self.data.packages.len()));
            title.push(format!("sort: {} ", value_name(&self.sort)));
            if self.running_only {
                title.push("running only ".to_owned());
            }
        }
        if !view.filter.is_empty() {
            title.push(format!("filter: {} ", view.filter));
        }
        if !view.marks.is_empty() {
            title.push(format!("{} marked ", view.marks.len()));
        }
        if !self.loaded[tab.index()] && self.data.len(tab) == 0 {
            title.push("loading… ".to_owned());
        }
        let table = Table::new(rows, widths)
            .header(Row::new(headers).style(Style::new().bold().underlined()))
            .row_highlight_style(Style::new().reversed())
            .column_spacing(1)
            .block(Block::bordered().title(title.join("· ")));
        frame.render_stateful_widget(table, area, &mut self.views[tab.index()].state);
    }

    fn draw_info(&self, frame: &mut Frame, area: Rect) {
        let version = self
            .data
            .version
            .map_or_else(|| "loading…".to_owned(), |version| version.to_string());
        let mut lines = vec![
            Line::from(vec!["JDownloader version: ".bold(), Span::raw(version)]),
            Line::default(),
            Line::from("API endpoints".bold()),
        ];
        lines.extend(
            self.data
                .api_help
                .lines()
                .map(|line| Line::from(line.to_owned())),
        );
        let paragraph = Paragraph::new(lines)
            .block(Block::bordered().title(" Info "))
            .scroll((self.info_scroll, 0));
        frame.render_widget(paragraph, area);
    }

    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let summary = format!(
            "{}{}",
            if self.pending > 0 { "working…  " } else { "" },
            self.data.summary()
        );
        let width = u16::try_from(summary.chars().count()).unwrap_or(u16::MAX);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(width)]).areas(area);
        if let Some(notice) = &self.notice {
            let style = if notice.error {
                Style::new().fg(Color::Red).bold()
            } else {
                Style::new().fg(Color::Green)
            };
            frame.render_widget(Line::styled(notice.text.clone(), style), left);
        }
        frame.render_widget(Line::from(summary).dim(), right);
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        if self.filtering {
            let line = Line::from(vec![
                "/".bold(),
                Span::raw(self.view().filter.clone()),
                "▏  Enter keep · Esc clear".dim(),
            ]);
            frame.render_widget(line, area);
            return;
        }
        let hints = match self.tab {
            Tab::Downloads => {
                "←→ fold · [] all · d remove · x reset · u resume · e/E enable · n rename · f folder · + priority · X extract · o sort · t running · w columns"
            }
            Tab::Grabber => "y confirm+start · Y confirm · v variant · D clear",
            Tab::Captchas => "y solve · x skip · w save image",
            Tab::Accounts => "n add · d remove · e/E enable · u refresh",
            Tab::Archives => "p password",
            Tab::Config => "e edit · v read value",
            Tab::Plugins => "p settings",
            Tab::Info => "j/k scroll",
        };
        let line = Line::from(format!(
            "{hints} │ a add · A containers · C cleanup · s/S start/stop · z/Z pause · / filter · ? help · q quit"
        ))
        .dim();
        frame.render_widget(line, area);
    }
}

fn draw_modal(frame: &mut Frame, modal: &Modal) {
    let area = frame.area();
    match modal {
        Modal::Form(form, _) => form.render(frame, area),
        Modal::Confirm(prompt, _) => {
            let width = u16::try_from(prompt.chars().count() + 6)
                .unwrap_or(u16::MAX)
                .max(30);
            let area = centered(area, width, 5);
            frame.render_widget(Clear, area);
            let text = vec![
                Line::from(prompt.clone()),
                Line::default(),
                Line::from("y/Enter yes · n/Esc no".dim()),
            ];
            let paragraph = Paragraph::new(text).block(Block::bordered().title(" Confirm "));
            frame.render_widget(paragraph, area);
        }
        Modal::Pick {
            title,
            options,
            state,
            ..
        } => {
            let width = options
                .iter()
                .map(|option| option.chars().count())
                .max()
                .unwrap_or_default()
                .max(title.len())
                + 6;
            let height = options.len() + 2;
            let area = centered(
                area,
                u16::try_from(width).unwrap_or(u16::MAX),
                u16::try_from(height).unwrap_or(u16::MAX),
            );
            frame.render_widget(Clear, area);
            let items: Vec<ListItem> = options
                .iter()
                .map(|option| ListItem::new(option.clone()))
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title(format!(" {title} ")))
                .highlight_style(Style::new().reversed());
            let mut state = *state;
            frame.render_stateful_widget(list, area, &mut state);
        }
        Modal::Text {
            title,
            body,
            scroll,
        } => {
            let width = area.width.saturating_sub(8);
            let height = area.height.saturating_sub(4);
            let area = centered(area, width, height);
            frame.render_widget(Clear, area);
            let paragraph = Paragraph::new(body.clone())
                .wrap(Wrap { trim: false })
                .scroll((*scroll, 0))
                .block(
                    Block::bordered()
                        .title(format!(" {title} "))
                        .title_bottom(" j/k scroll · Esc close ".dim()),
                );
            frame.render_widget(paragraph, area);
        }
    }
}

/// Builds the change a submitted form asks for, or says why the form is incomplete.
fn action(form: &Form, submit: &Submit) -> Result<Action> {
    Ok(match submit {
        Submit::AddLinks => {
            let mut urls = Vec::new();
            for url in unique_urls(form.text("URLs")) {
                let url = url?;
                if [Scheme::FTP, Scheme::HTTP, Scheme::HTTPS].contains(url.scheme()) {
                    urls.push((*url).to_owned());
                }
            }
            if urls.is_empty() {
                bail!("no ftp, http, or https URLs to add");
            }
            let dupes = dupes(form);
            let count = urls.len();
            let links = AddLinks {
                links: urls.join("\n"),
                autostart: form.toggle("Start"),
                package_name: form.optional("Package"),
                destination_folder: form.optional("Folder"),
                extract_password: form.optional("Extract password"),
                download_password: form.optional("Download password"),
            };
            Box::new(move |api| {
                if let Some(dupes) = dupes {
                    api.set_dupes_action(dupes)?;
                }
                let job = api.add_links(&links)?;
                Ok(format!("added {count} URLs as job {}", job.id))
            })
        }
        Submit::AddContainers => {
            let paths: Vec<PathBuf> = form
                .text("Files")
                .split(PATH_SEPARATOR)
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .collect();
            if paths.is_empty() {
                bail!("no container files to add");
            }
            let rm = form.toggle("Delete after adding");
            let dupes = dupes(form);
            Box::new(move |api| {
                if let Some(dupes) = dupes {
                    api.set_dupes_action(dupes)?;
                }
                let jobs = paths
                    .iter()
                    .map(|path| add_container(api, path, rm).map(|job| job.id.to_string()))
                    .collect::<Result<Vec<_>>>()?;
                Ok(format!("added containers as jobs {}", jobs.join(", ")))
            })
        }
        Submit::Cleanup { links, packages } => {
            let actions = CleanupActions {
                all: form.toggle("All"),
                disabled: form.toggle("Disabled"),
                failed: form.toggle("Failed"),
                finished: form.toggle("Finished"),
                offline: form.toggle("Offline"),
                dupe: form.toggle("Dupe"),
            };
            let mode = form
                .choice("Mode")
                .and_then(|index| CleanupMode::value_variants().get(index).copied())
                .unwrap_or(CleanupMode::LinksOnly);
            let (links, packages) = if form.choice("Scope") == Some(1) {
                (links.clone(), packages.clone())
            } else {
                (vec![], vec![])
            };
            Box::new(move |api| {
                for action in cleanup_actions(&actions) {
                    api.cleanup(&links, &packages, action, mode)?;
                }
                Ok("cleaned up the download list".to_owned())
            })
        }
        Submit::RenameLink(link) => {
            let (link, name) = (*link, required(form, "Name")?);
            Box::new(move |api| {
                api.rename_link(link, &name)
                    .map(|()| format!("renamed to {name}"))
            })
        }
        Submit::RenamePackage(package) => {
            let (package, name) = (*package, required(form, "Name")?);
            Box::new(move |api| {
                api.rename_package(package, &name)
                    .map(|()| format!("renamed to {name}"))
            })
        }
        Submit::SetFolder(packages) => {
            let (packages, folder) = (packages.clone(), required(form, "Folder")?);
            Box::new(move |api| {
                api.set_download_folder(&folder, &packages)
                    .map(|()| format!("set the download folder to {folder}"))
            })
        }
        Submit::Solve(captcha) => {
            let (captcha, answer) = (*captcha, required(form, "Answer")?);
            Box::new(move |api| {
                api.solve_captcha(captcha, &answer)
                    .map(|()| format!("answered captcha {captcha}"))
            })
        }
        Submit::SaveCaptcha(captcha) => {
            let captcha = *captcha;
            let file = form.optional("File").map(PathBuf::from);
            Box::new(move |api| {
                let image = api.captcha_image(captcha)?;
                let path = file.unwrap_or_else(|| {
                    PathBuf::from(format!("captcha-{captcha}.{}", image.extension))
                });
                write_file(&path, &image.data)?;
                Ok(format!("saved {}", path.display()))
            })
        }
        Submit::AddAccount => {
            let hoster = required(form, "Hoster")?;
            let username = required(form, "Username")?;
            let password = form.raw("Password").to_owned();
            if password.is_empty() {
                bail!("Password is required");
            }
            Box::new(move |api| {
                api.add_account(&hoster, &username, &password)
                    .map(|()| format!("added {username} on {hoster}"))
            })
        }
        Submit::ArchivePassword(archive) => {
            let archive = archive.clone();
            let password = form.raw("Password").to_owned();
            if password.is_empty() {
                bail!("Password is required");
            }
            Box::new(move |api| {
                api.set_archive_password(&archive, password)
                    .map(|()| format!("set the password of {archive}"))
            })
        }
        Submit::ConfigSet { interface, key } => {
            let value: Json = serde_json::from_str(form.text("JSON"))
                .map_err(|error| anyhow!("the value is not JSON: {error}"))?;
            let (interface, key) = (interface.clone(), key.clone());
            Box::new(move |api| {
                api.config_set(&interface, &key, &value)
                    .map(|()| format!("set {key} to {value}"))
            })
        }
    })
}

fn required(form: &Form, label: &str) -> Result<String> {
    form.optional(label)
        .ok_or_else(|| anyhow!("{label} is required"))
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

fn dupes_field() -> Field {
    let options = std::iter::once(DUPES_UNCHANGED.to_owned())
        .chain(DupesAction::value_variants().iter().map(value_name))
        .collect();
    Field::choice("Dupes", options, 0)
}

/// The dupes action a form picked; the first option leaves the setting unchanged.
fn dupes(form: &Form) -> Option<DupesAction> {
    let index = form.choice("Dupes")?.checked_sub(1)?;
    DupesAction::value_variants().get(index).copied()
}

fn plugin_settings(
    entries: Vec<crate::tools::jdownloader_remote::model::PluginConfigEntry>,
) -> String {
    if entries.is_empty() {
        return "no settings".to_owned();
    }
    entries
        .iter()
        .map(|entry| {
            format!(
                "{} {}\n  {} = {}",
                entry.interface_name, entry.storage, entry.key, entry.value
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn value_name(value: &impl ValueEnum) -> String {
    value
        .to_possible_value()
        .map(|value| value.get_name().to_owned())
        .unwrap_or_default()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn scroll(offset: u16, by: isize) -> u16 {
    let by = i32::try_from(by).unwrap_or(if by < 0 { i32::MIN } else { i32::MAX });
    u16::try_from((i32::from(offset)).saturating_add(by).max(0)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_describes_counts() {
        let selection = Selection {
            links: vec!["1".parse().unwrap(), "2".parse().unwrap()],
            packages: vec![],
        };
        assert_eq!(selection.describe(), "2 links");
        let selection = Selection {
            links: vec!["1".parse().unwrap()],
            packages: vec!["3".parse().unwrap()],
        };
        assert_eq!(selection.describe(), "1 link and 1 package");
    }

    #[test]
    fn dupes_defaults_to_unchanged() {
        let mut form = Form::new("test", vec![dupes_field()]);
        assert!(dupes(&form).is_none());
        form.key(KeyEvent::from(KeyCode::Right));
        assert!(matches!(dupes(&form), Some(DupesAction::Include)));
    }

    #[test]
    fn add_links_needs_a_url() {
        let form = Form::new("test", vec![Field::text("URLs", "not a url")]);
        assert!(action(&form, &Submit::AddLinks).is_err());
        let form = Form::new(
            "test",
            vec![Field::text(
                "URLs",
                "see https://example.com/a and ftp://x.org/b",
            )],
        );
        assert!(action(&form, &Submit::AddLinks).is_ok());
    }

    #[test]
    fn config_set_needs_json() {
        let submit = Submit::ConfigSet {
            interface: "i".to_owned(),
            key: "k".to_owned(),
        };
        let form = Form::new("test", vec![Field::text("JSON", "{oops")]);
        assert!(action(&form, &submit).is_err());
        let form = Form::new("test", vec![Field::text("JSON", "true")]);
        assert!(action(&form, &submit).is_ok());
    }

    #[test]
    fn scroll_stays_in_range() {
        assert_eq!(scroll(0, -5), 0);
        assert_eq!(scroll(3, 2), 5);
        assert_eq!(scroll(3, isize::MAX), u16::MAX);
    }
}
