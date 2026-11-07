use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use ratatui::layout::Constraint;
use serde::Serialize;

use crate::tools::jdownloader_remote::api::Api;
use crate::tools::jdownloader_remote::model::{
    Account, ArchiveStatus, CaptchaJob, ConfigEntry, CrawledLink, DownloadLink, FilePackage,
    LinkState, Plugin,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Downloads,
    Grabber,
    Captchas,
    Accounts,
    Archives,
    Config,
    Plugins,
    Info,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Downloads,
        Tab::Grabber,
        Tab::Captchas,
        Tab::Accounts,
        Tab::Archives,
        Tab::Config,
        Tab::Plugins,
        Tab::Info,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Downloads => "Downloads",
            Tab::Grabber => "Grabber",
            Tab::Captchas => "Captchas",
            Tab::Accounts => "Accounts",
            Tab::Archives => "Archives",
            Tab::Config => "Config",
            Tab::Plugins => "Plugins",
            Tab::Info => "Info",
        }
    }

    /// Whether the tab polls for changes while it is open.
    pub fn live(self) -> bool {
        !matches!(self, Tab::Config | Tab::Plugins | Tab::Info)
    }

    /// Whether rows of the tab can be marked for a bulk action.
    pub fn markable(self) -> bool {
        matches!(self, Tab::Downloads | Tab::Grabber | Tab::Accounts)
    }

    pub fn headers(self, columns: Columns) -> &'static [&'static str] {
        match self {
            Tab::Downloads if columns == Columns::Compact => &["", "name", "size"],
            Tab::Downloads => &[
                "",
                "%",
                "size",
                "speed",
                "eta",
                "priority",
                "status",
                "name",
                "host / folder",
            ],
            Tab::Grabber => &["availability", "size", "variant", "host", "name", "url"],
            Tab::Captchas => &["id", "hoster", "type", "remaining", "explain"],
            Tab::Accounts => &[
                "hoster", "username", "enabled", "valid", "traffic", "until", "error",
            ],
            Tab::Archives => &["id", "status", "type", "name"],
            Tab::Config => &["interface", "key", "type", "value"],
            Tab::Plugins => &["name", "version", "class"],
            Tab::Info => &[],
        }
    }

    pub fn widths(self, columns: Columns) -> Vec<Constraint> {
        use Constraint::{Fill, Length};
        match self {
            Tab::Downloads if columns == Columns::Compact => vec![Length(1), Fill(1), Length(10)],
            Tab::Downloads => vec![
                Length(1),
                Length(4),
                Length(19),
                Length(11),
                Length(8),
                Length(8),
                Fill(1),
                Fill(3),
                Fill(1),
            ],
            Tab::Grabber => vec![
                Length(12),
                Length(9),
                Length(12),
                Length(16),
                Fill(2),
                Fill(2),
            ],
            Tab::Captchas => vec![Length(14), Length(16), Length(20), Length(10), Fill(1)],
            Tab::Accounts => vec![
                Length(18),
                Fill(1),
                Length(7),
                Length(5),
                Length(21),
                Length(10),
                Fill(1),
            ],
            Tab::Archives => vec![Fill(1), Length(20), Length(12), Fill(2)],
            Tab::Config => vec![Fill(2), Fill(2), Length(10), Fill(2)],
            Tab::Plugins => vec![Fill(1), Length(10), Fill(2)],
            Tab::Info => vec![],
        }
    }
}

/// Which columns the download list shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Columns {
    /// State, name, and total size.
    Compact,
    /// Every column.
    Full,
}

impl Columns {
    pub fn toggled(self) -> Self {
        match self {
            Columns::Compact => Columns::Full,
            Columns::Full => Columns::Compact,
        }
    }
}

/// Whether a package row shows its links.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fold {
    Open,
    Closed,
}

/// A row of a tab, naming the item it shows by its index in [`Data`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Node {
    Package(usize, Fold),
    Link(usize),
    Grabber(usize),
    Captcha(usize),
    Account(usize),
    Archive(usize),
    Config(usize),
    Plugin(usize),
}

/// What the download tree shows besides the packages the user opened.
pub struct TreeFilter<'a> {
    /// Lowercase text a package or link must contain; empty matches everything.
    pub text: &'a str,
    pub running_only: bool,
}

/// A row's look, from the state of the item it shows.
#[derive(Clone, Copy)]
pub enum Tone {
    Normal,
    Disabled,
    Active,
    Finished,
    Failed,
}

pub enum Loaded {
    Downloads {
        packages: Vec<FilePackage>,
        links: Vec<DownloadLink>,
    },
    Grabber(Vec<CrawledLink>),
    Captchas(Vec<CaptchaJob>),
    Accounts(Vec<Account>),
    Archives(Vec<ArchiveStatus>),
    Config(Vec<ConfigEntry>),
    Plugins(Vec<Plugin>),
    Info {
        version: i64,
        api_help: String,
    },
}

impl Loaded {
    pub fn fetch(api: &Api, tab: Tab) -> Result<Self> {
        Ok(match tab {
            Tab::Downloads => Loaded::Downloads {
                packages: api.packages()?,
                links: api.downloads()?,
            },
            Tab::Grabber => Loaded::Grabber(api.grabber_links()?),
            Tab::Captchas => Loaded::Captchas(api.captchas()?),
            Tab::Accounts => Loaded::Accounts(api.accounts()?),
            Tab::Archives => Loaded::Archives(api.archives()?),
            Tab::Config => Loaded::Config(api.config_entries(None)?),
            Tab::Plugins => Loaded::Plugins(api.plugins()?),
            Tab::Info => Loaded::Info {
                version: api.version()?,
                api_help: api.api_help()?,
            },
        })
    }
}

#[derive(Default)]
pub struct Data {
    pub links: Vec<DownloadLink>,
    pub packages: Vec<FilePackage>,
    pub grabber: Vec<CrawledLink>,
    pub captchas: Vec<CaptchaJob>,
    pub accounts: Vec<Account>,
    pub archives: Vec<ArchiveStatus>,
    pub config: Vec<ConfigEntry>,
    pub plugins: Vec<Plugin>,
    pub version: Option<i64>,
    pub api_help: String,
    /// The state of each package in `packages`, rolled up from its links.
    package_states: Vec<LinkState>,
}

impl Data {
    pub fn store(&mut self, loaded: Loaded) {
        match loaded {
            Loaded::Downloads { packages, links } => {
                let mut children: HashMap<i64, Vec<LinkState>> = HashMap::new();
                for link in &links {
                    children
                        .entry(link.package_uuid.into())
                        .or_default()
                        .push(link.state());
                }
                self.package_states = packages
                    .iter()
                    .map(|package| {
                        let links = children
                            .get(&package.uuid.into())
                            .map_or(&[][..], Vec::as_slice);
                        LinkState::of_package(package.running == Some(true), links)
                    })
                    .collect();
                self.packages = packages;
                self.links = links;
            }
            Loaded::Grabber(grabber) => self.grabber = grabber,
            Loaded::Captchas(captchas) => self.captchas = captchas,
            Loaded::Accounts(accounts) => self.accounts = accounts,
            Loaded::Archives(archives) => self.archives = archives,
            Loaded::Config(config) => self.config = config,
            Loaded::Plugins(plugins) => self.plugins = plugins,
            Loaded::Info { version, api_help } => {
                self.version = Some(version);
                self.api_help = api_help;
            }
        }
    }

    /// The number of items a tab holds; the download list counts its links.
    pub fn len(&self, tab: Tab) -> usize {
        match tab {
            Tab::Downloads => self.links.len(),
            Tab::Grabber => self.grabber.len(),
            Tab::Captchas => self.captchas.len(),
            Tab::Accounts => self.accounts.len(),
            Tab::Archives => self.archives.len(),
            Tab::Config => self.config.len(),
            Tab::Plugins => self.plugins.len(),
            Tab::Info => 0,
        }
    }

    /// Every row of a flat tab, in order; the download list is a tree instead.
    pub fn rows(&self, tab: Tab) -> Vec<Node> {
        let node = match tab {
            Tab::Grabber => Node::Grabber,
            Tab::Captchas => Node::Captcha,
            Tab::Accounts => Node::Account,
            Tab::Archives => Node::Archive,
            Tab::Config => Node::Config,
            Tab::Plugins => Node::Plugin,
            Tab::Downloads | Tab::Info => return vec![],
        };
        (0..self.len(tab)).map(node).collect()
    }

    /// The download list as packages, each followed by its links when it is open.
    ///
    /// A package is open when `expanded` holds its key, when only running links
    /// are shown, or when it is shown only for links matching the filter. Links keep their order in `self.links`. Links whose
    /// package is not loaded follow the packages.
    pub fn tree(&self, expanded: &BTreeSet<String>, filter: &TreeFilter) -> Vec<Node> {
        let mut children: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, link) in self.links.iter().enumerate() {
            children
                .entry(link.package_uuid.into())
                .or_default()
                .push(index);
        }
        let shows = |link: usize, parent_matches: bool| {
            (!filter.running_only || self.links[link].running)
                && (parent_matches || self.matches(Node::Link(link), filter.text))
        };
        let mut rows = Vec::new();
        for (index, package) in self.packages.iter().enumerate() {
            let parent_matches = self.matches(Node::Package(index, Fold::Closed), filter.text);
            let links: Vec<usize> = children
                .remove(&package.uuid.into())
                .unwrap_or_default()
                .into_iter()
                .filter(|&link| shows(link, parent_matches))
                .collect();
            if links.is_empty() && (filter.running_only || !parent_matches) {
                continue;
            }
            // A package that fails the filter text is shown for its matching links.
            let open = expanded.contains(&self.key(Node::Package(index, Fold::Closed)))
                || filter.running_only
                || !parent_matches;
            let fold = if open { Fold::Open } else { Fold::Closed };
            rows.push(Node::Package(index, fold));
            if open {
                rows.extend(links.into_iter().map(Node::Link));
            }
        }
        let mut orphans: Vec<usize> = children
            .into_values()
            .flatten()
            .filter(|&link| shows(link, false))
            .collect();
        orphans.sort_unstable();
        rows.extend(orphans.into_iter().map(Node::Link));
        rows
    }

    /// Whether the row contains `filter`, which is lowercase; empty matches everything.
    pub fn matches(&self, node: Node, filter: &str) -> bool {
        filter.is_empty() || self.haystack(node).to_lowercase().contains(filter)
    }

    /// A key naming the row across reloads, for keeping marks and open packages.
    pub fn key(&self, node: Node) -> String {
        match node {
            Node::Package(i, _) => format!("package {}", self.packages[i].uuid),
            Node::Link(i) => format!("link {}", self.links[i].uuid),
            Node::Grabber(i) => self.grabber[i].uuid.to_string(),
            Node::Captcha(i) => self.captchas[i].id.to_string(),
            Node::Account(i) => self.accounts[i].uuid.to_string(),
            Node::Archive(i) => self.archives[i].archive_id.to_string(),
            Node::Config(i) => {
                let entry = &self.config[i];
                format!("{}\0{}", entry.interface_name, entry.key)
            }
            Node::Plugin(i) => self.plugins[i].class_name.clone(),
        }
    }

    /// The text a filter matches against.
    fn haystack(&self, node: Node) -> String {
        match node {
            Node::Package(i, _) => {
                let p = &self.packages[i];
                format!("{} {} {}", p.name, p.status, p.save_to)
            }
            Node::Link(i) => {
                let l = &self.links[i];
                format!("{} {} {}", l.name, l.status, l.host)
            }
            Node::Grabber(i) => {
                let l = &self.grabber[i];
                format!("{} {} {} {}", l.name, l.availability, l.host, l.url)
            }
            Node::Captcha(i) => {
                let c = &self.captchas[i];
                format!(
                    "{} {}",
                    c.hoster.as_deref().unwrap_or_default(),
                    c.explain.as_deref().unwrap_or_default()
                )
            }
            Node::Account(i) => {
                let a = &self.accounts[i];
                format!("{} {}", a.hostname, a.username)
            }
            Node::Archive(i) => {
                let a = &self.archives[i];
                format!(
                    "{} {}",
                    a.archive_id,
                    a.archive_name.as_deref().unwrap_or_default()
                )
            }
            Node::Config(i) => {
                let e = &self.config[i];
                format!("{} {}", e.interface_name, e.key)
            }
            Node::Plugin(i) => {
                let p = &self.plugins[i];
                format!("{} {}", p.display_name, p.class_name)
            }
        }
    }

    pub fn cells(&self, node: Node, columns: Columns) -> Vec<String> {
        match (node, columns) {
            (Node::Package(i, fold), Columns::Compact) => {
                let p = &self.packages[i];
                vec![
                    glyph(self.package_states[i]).to_owned(),
                    format!("{} {} ({})", arrow(fold), p.name, p.child_count),
                    bytes(p.bytes_total),
                ]
            }
            (Node::Link(i), Columns::Compact) => {
                let l = &self.links[i];
                vec![
                    glyph(l.state()).to_owned(),
                    format!("    {}", l.name),
                    bytes(l.bytes_total),
                ]
            }
            _ => self.full_cells(node),
        }
    }

    fn full_cells(&self, node: Node) -> Vec<String> {
        match node {
            Node::Package(i, fold) => {
                let p = &self.packages[i];
                vec![
                    glyph(self.package_states[i]).to_owned(),
                    format!("{}%", p.percent()),
                    format!("{}/{}", bytes(p.bytes_loaded), bytes(p.bytes_total)),
                    p.speed.map(speed).unwrap_or_default(),
                    eta(p.eta),
                    p.priority.clone().unwrap_or_default(),
                    p.status.clone(),
                    format!("{} {} ({})", arrow(fold), p.name, p.child_count),
                    p.save_to.clone(),
                ]
            }
            Node::Link(i) => {
                let l = &self.links[i];
                vec![
                    glyph(l.state()).to_owned(),
                    format!("{}%", l.percent()),
                    format!("{}/{}", bytes(l.bytes_loaded), bytes(l.bytes_total)),
                    speed(l.speed),
                    eta(l.eta),
                    l.priority.clone().unwrap_or_default(),
                    l.status.clone(),
                    format!("    {}", l.name),
                    l.host.clone(),
                ]
            }
            Node::Grabber(i) => {
                let l = &self.grabber[i];
                vec![
                    l.availability.clone(),
                    l.bytes_total.map(bytes).unwrap_or_default(),
                    l.variant
                        .as_ref()
                        .and_then(|variant| variant.name.clone())
                        .unwrap_or_default(),
                    l.host.clone(),
                    l.name.clone(),
                    l.url.clone(),
                ]
            }
            Node::Captcha(i) => {
                let c = &self.captchas[i];
                vec![
                    c.id.to_string(),
                    c.hoster.clone().unwrap_or_default(),
                    c.kind.clone().unwrap_or_default(),
                    c.remaining.map(|r| r.to_string()).unwrap_or_default(),
                    c.explain.clone().unwrap_or_default(),
                ]
            }
            Node::Account(i) => {
                let a = &self.accounts[i];
                vec![
                    a.hostname.clone(),
                    a.username.clone(),
                    yes_no(a.enabled),
                    yes_no(a.valid),
                    traffic(a.traffic_left, a.traffic_max),
                    a.valid_until.map(date).unwrap_or_default(),
                    a.error().unwrap_or_default().to_owned(),
                ]
            }
            Node::Archive(i) => {
                let a = &self.archives[i];
                vec![
                    a.archive_id.to_string(),
                    a.controller_status.clone().unwrap_or_default(),
                    a.kind.clone().unwrap_or_default(),
                    a.archive_name.clone().unwrap_or_default(),
                ]
            }
            Node::Config(i) => {
                let e = &self.config[i];
                vec![
                    e.interface_name.clone(),
                    e.key.clone(),
                    e.kind.clone(),
                    e.value.to_string(),
                ]
            }
            Node::Plugin(i) => {
                let p = &self.plugins[i];
                vec![
                    p.display_name.clone(),
                    p.version.clone(),
                    p.class_name.clone(),
                ]
            }
        }
    }

    pub fn tone(&self, node: Node) -> Tone {
        match node {
            Node::Package(i, _) => state_tone(self.packages[i].enabled, self.package_states[i]),
            Node::Link(i) => {
                let l = &self.links[i];
                state_tone(l.enabled, l.state())
            }
            Node::Grabber(i) => {
                let l = &self.grabber[i];
                if !l.enabled {
                    Tone::Disabled
                } else if l.availability == "OFFLINE" {
                    Tone::Failed
                } else {
                    Tone::Normal
                }
            }
            Node::Account(i) => {
                let a = &self.accounts[i];
                if !a.enabled {
                    Tone::Disabled
                } else if !a.valid || a.error().is_some() {
                    Tone::Failed
                } else {
                    Tone::Normal
                }
            }
            Node::Captcha(_) | Node::Archive(_) | Node::Config(_) | Node::Plugin(_) => Tone::Normal,
        }
    }

    /// The JSON of the row's item, as `--format json` shows it.
    pub fn details(&self, node: Node) -> String {
        match node {
            Node::Package(i, _) => pretty(&self.packages[i]),
            Node::Link(i) => pretty(&self.links[i]),
            Node::Grabber(i) => pretty(&self.grabber[i]),
            Node::Captcha(i) => pretty(&self.captchas[i]),
            Node::Account(i) => pretty(&self.accounts[i]),
            Node::Archive(i) => pretty(&self.archives[i]),
            Node::Config(i) => pretty(&self.config[i]),
            Node::Plugin(i) => pretty(&self.plugins[i]),
        }
    }

    /// The summary of running downloads for the status bar.
    pub fn summary(&self) -> String {
        let running = self.links.iter().filter(|link| link.running).count();
        let finished = self.links.iter().filter(|link| link.finished).count();
        let total: i64 = self.links.iter().map(|link| link.speed).sum();
        format!(
            "↓ {}  {running} running  {finished}/{} finished",
            speed(total),
            self.links.len()
        )
    }
}

fn state_tone(enabled: bool, state: LinkState) -> Tone {
    match (enabled, state) {
        (false, _) => Tone::Disabled,
        (true, LinkState::Running) => Tone::Active,
        (true, LinkState::Error) => Tone::Failed,
        (true, LinkState::Complete) => Tone::Finished,
        (true, LinkState::NotRunning) => Tone::Normal,
    }
}

fn arrow(fold: Fold) -> char {
    match fold {
        Fold::Open => '▾',
        Fold::Closed => '▸',
    }
}

/// The letter the state column shows.
fn glyph(state: LinkState) -> &'static str {
    match state {
        LinkState::Running => "R",
        LinkState::Error => "E",
        LinkState::Complete => "C",
        LinkState::NotRunning => "N",
    }
}

fn pretty(item: &impl Serialize) -> String {
    serde_json::to_string_pretty(item).unwrap_or_else(|error| error.to_string())
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_owned()
}

/// A byte count in binary units; JDownloader reports unknown sizes as negative.
pub fn bytes(count: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if count < 0 {
        return "?".to_owned();
    }
    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{count} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn speed(bytes_per_second: i64) -> String {
    if bytes_per_second <= 0 {
        String::new()
    } else {
        format!("{}/s", bytes(bytes_per_second))
    }
}

/// Seconds as `h:mm:ss`, or `m:ss` under an hour.
pub fn eta(seconds: Option<i64>) -> String {
    match seconds {
        Some(seconds) if seconds > 0 => {
            let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
            if hours > 0 {
                format!("{hours}:{minutes:02}:{seconds:02}")
            } else {
                format!("{minutes}:{seconds:02}")
            }
        }
        _ => String::new(),
    }
}

/// Traffic left of the maximum; JDownloader reports unlimited traffic as negative.
fn traffic(left: Option<i64>, max: Option<i64>) -> String {
    let side = |value: i64| {
        if value < 0 {
            "∞".to_owned()
        } else {
            bytes(value)
        }
    };
    match (left, max) {
        (Some(left), Some(max)) => format!("{}/{}", side(left), side(max)),
        (Some(left), None) => side(left),
        _ => String::new(),
    }
}

/// A millisecond timestamp as a date; JDownloader reports no expiry as non-positive.
fn date(milliseconds: i64) -> String {
    if milliseconds <= 0 {
        return String::new();
    }
    jiff::Timestamp::from_millisecond(milliseconds)
        .map(|timestamp| timestamp.strftime("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn package(uuid: i64, name: &str) -> FilePackage {
        serde_json::from_value(json!({
            "uuid": uuid, "name": name, "bytesLoaded": 0, "bytesTotal": 0, "childCount": 0,
            "enabled": true, "hosts": [], "saveTo": "/downloads", "status": "",
        }))
        .unwrap()
    }

    fn link(uuid: i64, package: i64, name: &str, running: bool) -> DownloadLink {
        serde_json::from_value(json!({
            "uuid": uuid, "name": name, "packageUUID": package, "bytesLoaded": 0,
            "bytesTotal": 0, "speed": 0, "enabled": true, "finished": false,
            "running": running, "host": "host", "url": "", "status": "", "addedDate": 0,
        }))
        .unwrap()
    }

    fn data() -> Data {
        Data {
            packages: vec![package(10, "movies"), package(20, "music")],
            links: vec![
                link(1, 10, "film.mkv", false),
                link(2, 20, "song.flac", true),
                link(3, 10, "trailer.mp4", false),
                link(4, 99, "stray.bin", false),
            ],
            ..Data::default()
        }
    }

    const ALL: TreeFilter = TreeFilter {
        text: "",
        running_only: false,
    };

    #[test]
    fn tree_shows_closed_packages_then_stray_links() {
        let rows = data().tree(&BTreeSet::new(), &ALL);
        assert_eq!(
            rows,
            [
                Node::Package(0, Fold::Closed),
                Node::Package(1, Fold::Closed),
                Node::Link(3),
            ]
        );
    }

    #[test]
    fn tree_opens_expanded_packages() {
        let data = data();
        let expanded = BTreeSet::from([data.key(Node::Package(0, Fold::Closed))]);
        let rows = data.tree(&expanded, &ALL);
        assert_eq!(
            rows,
            [
                Node::Package(0, Fold::Open),
                Node::Link(0),
                Node::Link(2),
                Node::Package(1, Fold::Closed),
                Node::Link(3),
            ]
        );
    }

    #[test]
    fn tree_filter_opens_packages_with_matching_links() {
        let filter = TreeFilter {
            text: "trailer",
            running_only: false,
        };
        let rows = data().tree(&BTreeSet::new(), &filter);
        assert_eq!(rows, [Node::Package(0, Fold::Open), Node::Link(2)]);
    }

    #[test]
    fn tree_filter_matching_a_package_keeps_it_closed() {
        let filter = TreeFilter {
            text: "music",
            running_only: false,
        };
        let rows = data().tree(&BTreeSet::new(), &filter);
        assert_eq!(rows, [Node::Package(1, Fold::Closed)]);
    }

    #[test]
    fn tree_running_only_shows_running_links() {
        let filter = TreeFilter {
            text: "",
            running_only: true,
        };
        let rows = data().tree(&BTreeSet::new(), &filter);
        assert_eq!(rows, [Node::Package(1, Fold::Open), Node::Link(1)]);
    }

    fn link_with(running: bool, finished: bool, advanced: serde_json::Value) -> DownloadLink {
        let mut value = serde_json::to_value(link(1, 10, "file", running)).unwrap();
        value["finished"] = json!(finished);
        value["advancedStatus"] = advanced;
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn link_state_running_wins() {
        let failed = json!({ "FinalLinkState": { "id": "FAILED" } });
        assert_eq!(link_with(true, false, failed).state(), LinkState::Running);
    }

    #[test]
    fn link_state_error_from_final_state_or_skip_reason() {
        let failed = json!({ "FinalLinkState": { "id": "OFFLINE" } });
        assert_eq!(link_with(false, false, failed).state(), LinkState::Error);
        let whirlpool = json!({ "FinalLinkState": { "id": "FAILED_WHIRLPOOL" } });
        assert_eq!(link_with(false, true, whirlpool).state(), LinkState::Error);
        let disk = json!({ "SkipReason": { "id": "DISK_FULL" } });
        assert_eq!(link_with(false, false, disk).state(), LinkState::Error);
    }

    #[test]
    fn link_state_manual_skip_is_not_running() {
        let manual = json!({ "SkipReason": { "id": "MANUAL" } });
        assert_eq!(
            link_with(false, false, manual).state(),
            LinkState::NotRunning
        );
    }

    #[test]
    fn link_state_complete_and_unknown_states() {
        let finished = json!({ "FinalLinkState": { "id": "FINISHED" } });
        assert_eq!(
            link_with(false, true, finished).state(),
            LinkState::Complete
        );
        let unknown = json!({ "FinalLinkState": { "id": "SOMETHING_NEW" }, "PluginProgress": {} });
        assert_eq!(
            link_with(false, false, unknown).state(),
            LinkState::NotRunning
        );
    }

    #[test]
    fn package_state_rolls_up_links() {
        use LinkState::{Complete, Error, NotRunning, Running};
        assert_eq!(LinkState::of_package(false, &[Complete, Running]), Running);
        assert_eq!(LinkState::of_package(true, &[]), Running);
        assert_eq!(LinkState::of_package(false, &[Complete, Error]), Error);
        assert_eq!(
            LinkState::of_package(false, &[Complete, Complete]),
            Complete
        );
        assert_eq!(
            LinkState::of_package(false, &[Complete, NotRunning]),
            NotRunning
        );
        assert_eq!(LinkState::of_package(false, &[]), NotRunning);
    }

    #[test]
    fn bytes_use_binary_units() {
        assert_eq!(bytes(-1), "?");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn eta_shows_hours_only_when_needed() {
        assert_eq!(eta(None), "");
        assert_eq!(eta(Some(-1)), "");
        assert_eq!(eta(Some(65)), "1:05");
        assert_eq!(eta(Some(3725)), "1:02:05");
    }

    #[test]
    fn traffic_shows_unlimited() {
        assert_eq!(traffic(Some(-1), Some(-1)), "∞/∞");
        assert_eq!(traffic(Some(1024), Some(2048)), "1.0 KiB/2.0 KiB");
        assert_eq!(traffic(None, None), "");
    }
}
