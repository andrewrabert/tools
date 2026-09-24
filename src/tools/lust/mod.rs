mod manage;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::dirs;
use crate::git::Git;
use crate::tools::Tool;
pub use crate::tools::lust::manage::LustManage;

const REMOTES: [&str; 2] = ["upstream", "origin"];
const METADATA_FILE: &str = ".lust.json";
const JUSTFILE: &str = "justfile";

#[derive(ClapArgs)]
#[command(about = "Run just against a personal per-project justfile stored outside the project")]
pub struct Lust {
    /// Arguments passed to just
    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<OsString>,
}

impl Tool for Lust {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

/// The directory holding every project directory.
struct LustDir {
    path: PathBuf,
}

impl LustDir {
    fn locate() -> Result<Self> {
        Ok(Self {
            path: dirs::config()?.join("just").join("lust"),
        })
    }

    fn project_dir(&self, key: &Key) -> PathBuf {
        self.path.join(&key.0)
    }

    /// Every project directory that carries a metadata file.
    fn projects(&self) -> Result<Vec<(PathBuf, Source)>> {
        let mut projects = Vec::new();
        let entries = match fs::read_dir(&self.path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(projects),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", self.path.display()));
            }
        };
        for entry in entries {
            let path = entry
                .with_context(|| format!("reading {}", self.path.display()))?
                .path();
            let Some(source) = Source::read(&path)? else {
                continue;
            };
            projects.push((path, source));
        }
        projects.sort_by(|(a, _), (b, _)| a.cmp(b));
        Ok(projects)
    }
}

/// What a project directory belongs to, as recorded in its metadata file.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    Remote(String),
    Path(PathBuf),
}

impl Source {
    fn key(&self) -> Key {
        let bytes = match self {
            Source::Remote(url) => url.as_bytes(),
            Source::Path(path) => path.as_os_str().as_bytes(),
        };
        Key(format!("{:x}", Sha256::digest(bytes)))
    }

    fn write(&self, project_dir: &Path) -> Result<()> {
        let path = project_dir.join(METADATA_FILE);
        let contents = serde_json::to_string_pretty(self)? + "\n";
        if fs::read_to_string(&path).is_ok_and(|current| current == contents) {
            return Ok(());
        }
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
    }

    /// The metadata in `project_dir`, or `None` when it has no metadata file.
    fn read(project_dir: &Path) -> Result<Option<Self>> {
        let path = project_dir.join(METADATA_FILE);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let source = serde_json::from_str(&contents)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(source))
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Remote(url) => f.write_str(url),
            Source::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

/// The name of a project directory under the lust directory.
struct Key(String);

/// The project lust runs against.
struct Project {
    source: Source,
    working_dir: PathBuf,
}

impl Project {
    fn resolve() -> Result<Self> {
        let git = Git::cwd();
        let Some(toplevel) = git_stdout(git, ["rev-parse", "--show-toplevel"])? else {
            let cwd = env::current_dir().context("reading the working directory")?;
            return Ok(Self {
                source: Source::Path(cwd.clone()),
                working_dir: cwd,
            });
        };
        let working_dir = PathBuf::from(toplevel);
        for remote in REMOTES {
            if let Some(url) = git_stdout(git, ["remote", "get-url", remote])? {
                return Ok(Self {
                    source: Source::Remote(url),
                    working_dir,
                });
            }
        }
        Ok(Self {
            source: Source::Path(working_dir.clone()),
            working_dir,
        })
    }
}

/// A project directory with its metadata and justfile in place.
struct ProjectDir {
    path: PathBuf,
    justfile: PathBuf,
}

impl ProjectDir {
    fn ensure(lust_dir: &LustDir, project: &Project) -> Result<Self> {
        let path = lust_dir.project_dir(&project.source.key());
        fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        project.source.write(&path)?;
        let justfile = path.join(JUSTFILE);
        if !justfile.exists() {
            fs::write(&justfile, "").with_context(|| format!("writing {}", justfile.display()))?;
            eprintln!("Created {}", justfile.display());
        }
        Ok(Self { path, justfile })
    }
}

/// Trimmed stdout of a git command, or `None` when git exits unsuccessfully.
fn git_stdout<const N: usize>(git: Git, args: [&str; N]) -> Result<Option<String>> {
    let output = git
        .command(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .context("running git")?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8(output.stdout).context("git output is not UTF-8")?;
    Ok(Some(stdout.trim_end_matches('\n').to_owned()))
}

fn run(args: Lust) -> Result<()> {
    let lust_dir = LustDir::locate()?;
    let project = Project::resolve()?;
    let project_dir = ProjectDir::ensure(&lust_dir, &project)?;

    let mut command = Command::new("just");
    command
        .arg("--justfile")
        .arg(&project_dir.justfile)
        .arg("--working-directory")
        .arg(&project.working_dir)
        .arg("--default-list")
        .args(&args.args)
        .env("JUST_JUSTFILE", &project_dir.justfile)
        .env("JUST_WORKING_DIRECTORY", &project.working_dir)
        .env("LUST_REPO_DIR", &project_dir.path);
    Err(command.exec()).context("running just")
}
