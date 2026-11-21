//! What every sandbox grants, and the steps that turn settings into one backend's command.
use std::collections::{BTreeSet, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

use super::noted::Endpoint;
use super::paths;
use super::settings::{Access, Bind, Settings};
use crate::{dirs, which};

/// Variables that would point the sandboxed program at the wrapper's notes.
const SANDBOX_UNSETENV: [&str; 5] = [
    "NOTED_DIR",
    "NOTED_ENV_FILE",
    "NOTED_POLICY",
    "NOTED_SCOPE",
    "NOTED_SOURCE",
];

/// The machine the wrapper runs on.
pub struct Host {
    pub home: PathBuf,
    pub work_dir: PathBuf,
    pub uid: u32,
}

impl Host {
    pub fn current() -> Result<Host> {
        Ok(Host {
            home: dirs::home()?,
            work_dir: paths::resolve(Path::new(".")),
            uid: nix::unistd::getuid().as_raw(),
        })
    }

    /// Read-only paths every backend grants.
    pub fn shared_ro(&self) -> Vec<PathBuf> {
        let dotfiles = self.home.join(".dotfiles/public");
        let git_config = self.home.join(".config/git");
        vec![
            "/bin".into(),
            "/opt".into(),
            "/usr".into(),
            self.home.join(".local/bin"),
            dotfiles.join(".local/bin"),
            dotfiles.join("scripts"),
            self.home.join(".cargo"),
            self.home.join(".rustup"),
            paths::resolve(&git_config),
            git_config,
        ]
    }
}

/// The mounts and environment a backend grants every program.
pub struct Defaults {
    pub ro: Vec<PathBuf>,
    pub rw: Vec<PathBuf>,
    /// Variables passed through from the wrapper when it has them.
    pub inherit: Vec<&'static str>,
    pub set: Vec<(&'static str, OsString)>,
}

/// One sandbox backend: a command plus the mounts and environment it enforces.
pub trait Sandbox: Sized {
    /// The program that enforces the sandbox.
    const BIN: &'static str;
    /// Whether every bind source must already exist on the host.
    const REQUIRES_EXISTING_SOURCES: bool;

    /// A sandbox with the platform plumbing every mount relies on.
    fn new(host: &Host) -> Result<Self>;

    fn defaults(host: &Host) -> Defaults;

    /// The bind for one default mount, or None to skip it on this platform.
    fn default_bind(&mut self, path: &Path, access: Access) -> Option<Bind> {
        Some(Bind {
            source: path.to_owned(),
            target: None,
            access,
        })
    }

    /// Emits anything the binds deferred.
    fn finish_mounts(&mut self) {}

    fn mount(&mut self, source: &Path, destination: &Path, access: Access) -> Result<()>;

    /// Makes `path` unreadable however broadly its directory was granted.
    fn hide(&mut self, path: &Path);

    /// Hides an existing file or directory tree.
    fn exclude(&mut self, path: &Path);

    fn setenv(&mut self, name: &OsStr, value: &OsStr);

    fn unsetenv(&mut self, name: &OsStr);

    /// Sets the sandboxed program's working directory.
    fn chdir(&mut self, path: &Path);

    /// Appends to the program and its arguments.
    fn arg(&mut self, arg: &OsStr);

    /// The full command to launch.
    fn command(&self) -> Command;
}

/// Whether the sandboxed program reaches the wrapper's notes server.
pub enum Noted<'a> {
    Disabled,
    Enabled {
        endpoint: &'a Endpoint,
        /// The NOTED_SOURCE the program reports.
        source: &'a str,
    },
}

/// Everything one run puts in its sandbox.
pub struct Invocation<'a> {
    pub settings: &'a Settings,
    pub noted: Noted<'a>,
    pub program: &'a Path,
    pub program_args: &'a [OsString],
    /// The project file, hidden from the program.
    pub config: Option<&'a Path>,
}

/// The extra grants a known program needs.
#[derive(Default)]
struct ProgramConfig {
    rw: Vec<PathBuf>,
    inherit: &'static [&'static str],
    /// Arguments that keep the program from looking for its notes server.
    no_noted_args: &'static [&'static str],
}

impl ProgramConfig {
    /// Paths a platform lacks are skipped by backends that require existing sources and are
    /// harmless rules on backends that do not.
    fn for_program(program: &Path, host: &Host) -> ProgramConfig {
        let home = &host.home;
        let var = |name| env::var_os(name).filter(|value| !value.is_empty());
        match program.file_name().and_then(OsStr::to_str) {
            Some("claude") => {
                let (config_dir, claude_json) = match var("CLAUDE_CONFIG_DIR") {
                    Some(dir) => {
                        let dir = PathBuf::from(dir);
                        let json = dir.join(".claude.json");
                        (dir, json)
                    }
                    None => (home.join(".claude"), home.join(".claude.json")),
                };
                ProgramConfig {
                    rw: vec![
                        config_dir,
                        claude_json,
                        // claude writes here whatever TMPDIR says, so the path has to be
                        // granted by name. It is shared with every unsandboxed claude this user
                        // runs.
                        PathBuf::from(format!("/private/tmp/claude-{}", host.uid)),
                        // where claude keeps its token on macOS
                        home.join("Library/Keychains"),
                    ],
                    inherit: &["CLAUDE_CONFIG_DIR"],
                    no_noted_args: &["--disallowed-tools=mcp__noted__*"],
                }
            }
            Some("codex") => ProgramConfig {
                rw: vec![var("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from)],
                inherit: &["CODEX_HOME"],
                ..ProgramConfig::default()
            },
            Some("opencode") => {
                let dir = home.join(".config/opencode");
                ProgramConfig {
                    rw: vec![paths::resolve(&dir), dir],
                    ..ProgramConfig::default()
                }
            }
            Some("pi") => ProgramConfig {
                rw: vec![home.join(".pi"), paths::resolve(&home.join(".pi/agent"))],
                no_noted_args: &["--no-noted"],
                ..ProgramConfig::default()
            },
            _ => ProgramConfig::default(),
        }
    }
}

/// Every distinct file PATH resolves `name` to.
fn which_all(name: &str) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    which::all(name)
        .iter()
        .map(|path| paths::resolve(path))
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

/// Adds the invocation's mounts, environment, and program to `sandbox`.
pub fn build<S: Sandbox>(sandbox: &mut S, host: &Host, invocation: &Invocation) -> Result<()> {
    let settings = invocation.settings;
    let work_dir = &host.work_dir;
    let defaults = S::defaults(host);
    let program = ProgramConfig::for_program(invocation.program, host);

    let mut ro = defaults.ro;
    let mut rw = defaults.rw;
    rw.extend(program.rw);
    if let Some(tmux) = env::var_os("TMUX").filter(|value| !value.is_empty()) {
        let socket = tmux
            .as_bytes()
            .split(|&byte| byte == b',')
            .next()
            .unwrap_or_default();
        rw.push(PathBuf::from(OsStr::from_bytes(socket)));
    }
    if settings.ro {
        ro.push(work_dir.clone());
        rw.retain(|path| path != work_dir);
    }

    let mut binds = Vec::new();
    for (paths, access) in [(ro, Access::ReadOnly), (rw, Access::ReadWrite)] {
        for path in paths {
            // A read-write working directory mount already provides everything beneath it.
            if !settings.ro && path != *work_dir && path.starts_with(work_dir) {
                continue;
            }
            binds.extend(sandbox.default_bind(&path, access));
        }
    }
    binds.extend(settings.bind.iter().cloned());
    if S::REQUIRES_EXISTING_SOURCES
        && let Some(missing) = binds.iter().find(|bind| !bind.source.exists())
    {
        bail!("bind source does not exist: {}", missing.source.display());
    }
    let (inside, outside): (Vec<_>, Vec<_>) = binds
        .into_iter()
        .partition(|bind| bind.destination().starts_with(work_dir));
    for bind in &outside {
        sandbox.mount(&bind.source, bind.destination(), bind.access)?;
    }
    // The only hole to the notes, read-only working directory or not, and the socket file alone.
    if let Noted::Enabled { endpoint, .. } = invocation.noted {
        let socket = endpoint.socket();
        sandbox.mount(socket, socket, Access::ReadWrite)?;
    }
    for bind in &inside {
        sandbox.mount(&bind.source, bind.destination(), bind.access)?;
    }
    sandbox.finish_mounts();

    // After every broader mount, so the project file cannot be read.
    if let Some(config) = invocation.config {
        sandbox.hide(config);
    }
    for name in &settings.disable {
        let matches = which_all(name);
        if matches.is_empty() {
            bail!("cannot disable {name}: not found on PATH");
        }
        for path in &matches {
            sandbox.hide(path);
        }
    }
    for path in &settings.exclude {
        if !path.exists() {
            bail!("cannot exclude nonexistent path: {}", path.display());
        }
        sandbox.exclude(path);
    }

    // An absent variable stays absent; an empty value is not the same thing.
    for name in defaults.inherit.iter().chain(program.inherit) {
        if let Some(value) = env::var_os(name) {
            sandbox.setenv(OsStr::new(name), &value);
        }
    }
    for name in SANDBOX_UNSETENV {
        sandbox.unsetenv(OsStr::new(name));
    }
    for (name, value) in &defaults.set {
        sandbox.setenv(OsStr::new(name), value);
    }
    if let Noted::Enabled { endpoint, .. } = invocation.noted {
        sandbox.setenv(OsStr::new("NOTED_URL"), endpoint.url());
    }
    for (name, value) in &settings.env {
        sandbox.setenv(OsStr::new(name), OsStr::new(value));
    }
    match invocation.noted {
        Noted::Disabled => {
            let noted = |name: &OsStr| name.as_bytes().starts_with(b"NOTED_");
            let names: BTreeSet<OsString> = SANDBOX_UNSETENV
                .iter()
                .chain(&["NOTED_URL"])
                .map(OsString::from)
                .chain(
                    env::vars_os()
                        .map(|(name, _)| name)
                        .filter(|name| noted(name)),
                )
                .chain(
                    settings
                        .env
                        .keys()
                        .map(OsString::from)
                        .filter(|name| noted(name)),
                )
                .collect();
            for name in &names {
                sandbox.unsetenv(name);
            }
        }
        Noted::Enabled { source, .. } => {
            sandbox.setenv(OsStr::new("NOTED_SOURCE"), OsStr::new(source));
        }
    }

    sandbox.chdir(work_dir);
    sandbox.arg(invocation.program.as_os_str());
    if let Noted::Disabled = invocation.noted {
        for arg in program.no_noted_args {
            sandbox.arg(OsStr::new(arg));
        }
    }
    for arg in invocation.program_args {
        sandbox.arg(arg);
    }
    Ok(())
}
