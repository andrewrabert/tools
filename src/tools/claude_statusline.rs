use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use crossterm::style::Stylize;
use serde::Deserialize;

use crate::dirs;
use crate::tools::Tool;

const SESSION_SHORT: usize = 6;

#[derive(ClapArgs)]
#[command(about = "Render the Claude Code status line from the hook json on stdin")]
pub struct ClaudeStatusline {}

#[derive(Deserialize)]
struct Hook {
    session_id: String,
    transcript_path: PathBuf,
    cwd: PathBuf,
    workspace: Workspace,
}

#[derive(Deserialize)]
struct Workspace {
    project_dir: PathBuf,
}

/// The hook's paths made absolute against the process cwd, the way `Path.absolute()` does.
struct Session {
    id: String,
    transcript: PathBuf,
    cwd: PathBuf,
    project: PathBuf,
    home: PathBuf,
}

impl Tool for ClaudeStatusline {
    fn run(self) -> ExitCode {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

impl TryFrom<Hook> for Session {
    type Error = anyhow::Error;

    fn try_from(hook: Hook) -> Result<Self> {
        let home = dirs::home()?;
        Ok(Session {
            id: hook.session_id,
            transcript: absolute(&hook.transcript_path)?,
            cwd: absolute(&hook.cwd)?,
            project: absolute(&hook.workspace.project_dir)?,
            home: absolute(&home)?,
        })
    }
}

impl Session {
    fn project(&self) -> String {
        match self.project.strip_prefix(&self.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => self.project.display().to_string(),
        }
    }

    fn cwd(&self) -> Result<String> {
        if self.cwd == self.project {
            return Ok(String::new());
        }
        let rest = self.cwd.strip_prefix(&self.project).with_context(|| {
            format!(
                "{} is not under {}",
                self.cwd.display(),
                self.project.display()
            )
        })?;
        Ok(format!("{}/", rest.display()))
    }

    fn short_id(&self) -> String {
        self.id.chars().take(SESSION_SHORT).collect()
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("resolving {}", path.display()))
}

fn grey(text: &str) -> String {
    text.dark_grey().to_string()
}

fn fg(text: &str) -> String {
    text.white().to_string()
}

fn link(target: &Path, text: &str) -> String {
    format!(
        "\x1b]8;;file://{}\x1b\\{text}\x1b]8;;\x1b\\",
        target.display()
    )
}

fn run() -> Result<()> {
    let hook: Hook = serde_json::from_reader(io::stdin().lock()).context("parsing hook json")?;
    let session = Session::try_from(hook)?;
    let project = format!("{}/", session.project());
    let path = link(
        &session.cwd,
        &format!("{}{}", grey(&project), fg(&session.cwd()?)),
    );
    let id = link(&session.transcript, &grey(&session.short_id()));
    println!("{id} {path}");
    Ok(())
}
