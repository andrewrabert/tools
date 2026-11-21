//! Sandbox settings, merged from a project file and the command line.
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};

use super::paths;
use crate::atomic_write;

/// The project file, found at or above the working directory.
pub const CONFIG_NAME: &str = ".bwrapped.conf";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// One bind mount: `source` on the host, seen at `target` (or `source`) in the sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "BindEntry", into = "BindEntry")]
pub struct Bind {
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub access: Access,
}

impl Bind {
    pub fn destination(&self) -> &Path {
        self.target.as_deref().unwrap_or(&self.source)
    }

    /// This bind with its source resolved against `directory`.
    fn relative_to(self, directory: &Path) -> Bind {
        Bind {
            source: paths::resolve(&directory.join(&self.source)),
            ..self
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BindParseError {
    UnsupportedEscape(char),
    IncompleteEscape,
    EmptySource,
    TooManyParts,
    InvalidMode(String),
}

impl fmt::Display for BindParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindParseError::UnsupportedEscape(char) => write!(f, "unsupported escape \\{char}"),
            BindParseError::IncompleteEscape => f.write_str("bind ends with an incomplete escape"),
            BindParseError::EmptySource => f.write_str("bind source cannot be empty"),
            BindParseError::TooManyParts => {
                f.write_str("bind must be SOURCE[:DESTINATION[:ro|rw]]")
            }
            BindParseError::InvalidMode(mode) => {
                write!(f, "bind mode must be 'ro' or 'rw', got {mode:?}")
            }
        }
    }
}

impl std::error::Error for BindParseError {}

/// `SOURCE[:DESTINATION[:ro|rw]]`, where `\:` and `\\` escape a colon and a backslash.
impl FromStr for Bind {
    type Err = BindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = vec![String::new()];
        let mut chars = value.chars();
        while let Some(char) = chars.next() {
            match char {
                '\\' => match chars.next() {
                    Some(escaped @ (':' | '\\')) => parts.last_mut().unwrap().push(escaped),
                    Some(other) => return Err(BindParseError::UnsupportedEscape(other)),
                    None => return Err(BindParseError::IncompleteEscape),
                },
                ':' => parts.push(String::new()),
                other => parts.last_mut().unwrap().push(other),
            }
        }
        if parts.len() > 3 {
            return Err(BindParseError::TooManyParts);
        }
        let mut parts = parts.into_iter();
        let source = parts.next().unwrap_or_default();
        if source.is_empty() {
            return Err(BindParseError::EmptySource);
        }
        let target = parts.next().filter(|target| !target.is_empty());
        let access = match parts.next().as_deref() {
            None | Some("rw") => Access::ReadWrite,
            Some("ro") => Access::ReadOnly,
            Some(other) => return Err(BindParseError::InvalidMode(other.to_owned())),
        };
        Ok(Bind {
            source: source.into(),
            target: target.map(PathBuf::from),
            access,
        })
    }
}

/// A bind as a project file writes it: a `SOURCE[:DESTINATION[:ro|rw]]` string or a table.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum BindEntry {
    Spec(String),
    Table(BindTable),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BindTable {
    source: PathBuf,
    #[serde(default)]
    read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<PathBuf>,
}

impl TryFrom<BindEntry> for Bind {
    type Error = BindParseError;

    fn try_from(entry: BindEntry) -> Result<Self, Self::Error> {
        match entry {
            BindEntry::Spec(spec) => spec.parse(),
            BindEntry::Table(table) => Ok(Bind {
                source: table.source,
                target: table.target.filter(|target| !target.as_os_str().is_empty()),
                access: if table.read_only {
                    Access::ReadOnly
                } else {
                    Access::ReadWrite
                },
            }),
        }
    }
}

impl From<Bind> for BindEntry {
    fn from(bind: Bind) -> Self {
        BindEntry::Table(BindTable {
            source: bind.source,
            read_only: bind.access == Access::ReadOnly,
            target: bind.target,
        })
    }
}

/// Sandbox options the command line shares with the project file.
#[derive(Args)]
pub struct SandboxArgs {
    /// Mount the working directory read-only
    #[arg(long, overrides_with = "no_ro")]
    ro: bool,
    /// Mount the working directory read-write, whatever the project file says
    #[arg(long, overrides_with = "ro")]
    no_ro: bool,
    /// Bind SOURCE[:DESTINATION[:ro|rw]] (repeatable)
    #[arg(short, long, value_name = "SPEC")]
    bind: Vec<Bind>,
    /// Hide PATH inside the sandbox (repeatable)
    #[arg(short = 'x', long, value_name = "PATH")]
    exclude: Vec<PathBuf>,
    /// Disable every PATH match for NAME (repeatable)
    #[arg(short, long, value_name = "NAME")]
    disable: Vec<String>,
    /// Set KEY=VALUE (repeatable)
    #[arg(short, long, value_name = "KEY=VALUE", value_parser = parse_env)]
    env: Vec<(String, String)>,
    /// Restrict the notes server to this scope
    #[arg(long, value_name = "SCOPE")]
    noted_scope: Option<String>,
    /// Set NOTED_SOURCE (defaults to PROGRAM)
    #[arg(long, value_name = "SOURCE")]
    noted_source: Option<String>,
}

fn parse_env(value: &str) -> Result<(String, String), String> {
    match value.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_owned(), value.to_owned())),
        _ => Err(format!("expected KEY=VALUE, got {value:?}")),
    }
}

impl SandboxArgs {
    fn ro(&self) -> Option<bool> {
        match (self.ro, self.no_ro) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        }
    }
}

/// The project file's contents, and the effective settings of one invocation.
#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub ro: bool,
    #[serde(default)]
    pub bind: Vec<Bind>,
    #[serde(default)]
    pub exclude: Vec<PathBuf>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noted_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noted_source: Option<String>,
}

/// The nearest project file at or above `directory`.
pub fn find_config(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .map(|parent| parent.join(CONFIG_NAME))
        .find(|candidate| candidate.is_file())
}

impl Settings {
    /// The project file at `config`, if any, overridden by the command line.
    ///
    /// Command-line scalars replace the file's; collections hold the file's entries, then the
    /// command line's. Relative paths resolve against the file's directory or `work_dir`, and
    /// of two binds with one destination the later wins.
    pub fn resolve(config: Option<&Path>, cli: SandboxArgs, work_dir: &Path) -> Result<Settings> {
        let (project, project_dir) = match config {
            None => (Settings::default(), work_dir),
            Some(path) => (Settings::load(path)?, path.parent().unwrap_or(work_dir)),
        };
        let binds = project
            .bind
            .into_iter()
            .map(|bind| bind.relative_to(project_dir))
            .chain(
                cli.bind
                    .iter()
                    .cloned()
                    .map(|bind| bind.relative_to(work_dir)),
            );
        let exclude = project
            .exclude
            .iter()
            .map(|path| paths::resolve(&project_dir.join(path)))
            .chain(
                cli.exclude
                    .iter()
                    .map(|path| paths::resolve(&work_dir.join(path))),
            )
            .collect();
        let mut env = project.env;
        env.extend(cli.env.iter().cloned());
        Ok(Settings {
            ro: cli.ro().unwrap_or(project.ro),
            bind: unique_destinations(binds),
            exclude,
            disable: project.disable.into_iter().chain(cli.disable).collect(),
            env,
            noted_scope: cli.noted_scope.or(project.noted_scope),
            noted_source: cli.noted_source.or(project.noted_source),
        })
    }

    fn load(path: &Path) -> Result<Settings> {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot load {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("cannot load {}", path.display()))
    }

    /// Writes these settings as TOML to `path`, or to the project file in it if it is a
    /// directory.
    pub fn write(&self, path: &Path) -> Result<()> {
        let path = if path.is_dir() {
            path.join(CONFIG_NAME)
        } else {
            path.to_owned()
        };
        let text = toml::to_string(self).context("serializing the settings")?;
        let temp = atomic_write::sibling(".bwrapped", &path)?;
        fs::write(&temp, text).with_context(|| format!("cannot write {}", path.display()))?;
        atomic_write::replace(temp, &path)
    }
}

/// `binds` with every bind dropped that a later one at the same destination replaces.
fn unique_destinations(binds: impl DoubleEndedIterator<Item = Bind>) -> Vec<Bind> {
    let mut seen = HashSet::new();
    let mut unique: Vec<Bind> = binds
        .rev()
        .filter(|bind| seen.insert(bind.destination().to_owned()))
        .collect();
    unique.reverse();
    unique
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        sandbox: SandboxArgs,
    }

    fn cli(args: &[&str]) -> SandboxArgs {
        Cli::try_parse_from(std::iter::once("test").chain(args.iter().copied()))
            .unwrap()
            .sandbox
    }

    fn bind(source: &str, target: Option<&str>, access: Access) -> Bind {
        Bind {
            source: source.into(),
            target: target.map(PathBuf::from),
            access,
        }
    }

    #[test]
    fn bind_specs_parse() {
        assert_eq!("/a".parse(), Ok(bind("/a", None, Access::ReadWrite)));
        assert_eq!("/a::ro".parse(), Ok(bind("/a", None, Access::ReadOnly)));
        assert_eq!(
            "/a\\:b:/c:rw".parse(),
            Ok(bind("/a:b", Some("/c"), Access::ReadWrite))
        );
        assert_eq!("/a\\\\".parse(), Ok(bind("/a\\", None, Access::ReadWrite)));
    }

    #[test]
    fn bad_bind_specs_are_rejected() {
        assert_eq!("".parse::<Bind>(), Err(BindParseError::EmptySource));
        assert_eq!(":/b".parse::<Bind>(), Err(BindParseError::EmptySource));
        assert_eq!(
            "/a:/b:ro:x".parse::<Bind>(),
            Err(BindParseError::TooManyParts)
        );
        assert_eq!(
            "/a:/b:wr".parse::<Bind>(),
            Err(BindParseError::InvalidMode("wr".into()))
        );
        assert_eq!(
            "/a\\n".parse::<Bind>(),
            Err(BindParseError::UnsupportedEscape('n'))
        );
        assert_eq!(
            "/a\\".parse::<Bind>(),
            Err(BindParseError::IncompleteEscape)
        );
    }

    #[test]
    fn project_file_takes_strings_and_tables() {
        let settings: Settings = toml::from_str(
            r#"
            ro = true
            bind = ["/a::ro", { source = "/b", target = "/c", read_only = true }, { source = "/d", target = "" }]
            noted_scope = "dev"
            "#,
        )
        .unwrap();
        assert!(settings.ro);
        assert_eq!(
            settings.bind,
            [
                bind("/a", None, Access::ReadOnly),
                bind("/b", Some("/c"), Access::ReadOnly),
                bind("/d", None, Access::ReadWrite),
            ]
        );
        assert_eq!(settings.noted_scope.as_deref(), Some("dev"));
    }

    #[test]
    fn project_file_rejects_unknown_keys() {
        assert!(toml::from_str::<Settings>("rw = true").is_err());
        assert!(toml::from_str::<Settings>(r#"bind = [{ source = "/a", mode = "ro" }]"#).is_err());
    }

    #[test]
    fn command_line_overrides_and_extends_project_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let work_dir = root.join("work");
        fs::create_dir(&work_dir).unwrap();
        let config = root.join(CONFIG_NAME);
        fs::write(
            &config,
            r#"
            ro = true
            bind = ["rel", "/x:/shared"]
            exclude = ["secret"]
            disable = ["git"]
            env = { A = "file", B = "file" }
            noted_scope = "file"
            "#,
        )
        .unwrap();

        let settings = Settings::resolve(
            Some(&config),
            cli(&[
                "--no-ro",
                "-b",
                "/y:/shared:ro",
                "-x",
                "hidden",
                "-d",
                "ssh",
                "-e",
                "B=cli",
                "--noted-source",
                "cli",
            ]),
            &work_dir,
        )
        .unwrap();

        assert_eq!(
            settings,
            Settings {
                ro: false,
                bind: vec![
                    bind(root.join("rel").to_str().unwrap(), None, Access::ReadWrite),
                    bind("/y", Some("/shared"), Access::ReadOnly),
                ],
                exclude: vec![root.join("secret"), work_dir.join("hidden")],
                disable: vec!["git".into(), "ssh".into()],
                env: [("A", "file"), ("B", "cli")]
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .into(),
                noted_scope: Some("file".into()),
                noted_source: Some("cli".into()),
            }
        );
    }

    #[test]
    fn written_settings_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            ro: true,
            bind: vec![
                bind("/a", None, Access::ReadWrite),
                bind("/b", Some("/c"), Access::ReadOnly),
            ],
            exclude: vec!["/e".into()],
            disable: vec!["git".into()],
            env: [("K".to_owned(), "V".to_owned())].into(),
            noted_scope: None,
            noted_source: Some("me".into()),
        };
        settings.write(dir.path()).unwrap();
        let path = dir.path().join(CONFIG_NAME);
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert_eq!(find_config(&dir.path().join("sub")), Some(path));
    }
}
