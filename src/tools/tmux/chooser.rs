use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::picker::{self, Matching};
use crate::tmux;
use crate::tools::Tool;
use crate::tools::tmux::exit_code;

#[derive(ClapArgs)]
#[command(about = "Pick a tmux window or labeled pane and switch to it")]
pub struct Chooser {}

impl Tool for Chooser {
    fn run(self) -> ExitCode {
        exit_code(run())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    window: u32,
    pane: Option<u32>,
    label: Option<String>,
}

impl Entry {
    fn parse(line: &str) -> Result<Self> {
        let (head, label) = line.split_once(':').unwrap_or((line, ""));
        let (window, pane) = match head.split_once('.') {
            Some((window, pane)) => (window, Some(pane)),
            None => (head, None),
        };
        Ok(Entry {
            window: window
                .parse()
                .with_context(|| format!("parsing the window of {line:?}"))?,
            pane: pane
                .map(str::parse)
                .transpose()
                .with_context(|| format!("parsing the pane of {line:?}"))?,
            label: Some(label)
                .filter(|label| !label.is_empty())
                .map(str::to_owned),
        })
    }

    fn select(&self) -> Result<()> {
        tmux::run(["select-window", "-t", &format!(":{}", self.window)])
            .map_err(anyhow::Error::msg)?;
        if let Some(pane) = self.pane {
            tmux::run(["select-pane", "-t", &format!(":.{pane}")]).map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}

fn run() -> Result<()> {
    let windows = String::from_utf8(
        tmux::capture(["list-windows", "-F", "#I#{?@renamed,:#W,}"]).map_err(anyhow::Error::msg)?,
    )?;
    let panes = String::from_utf8(
        tmux::capture([
            "list-panes",
            "-s",
            "-F",
            "#{?@pane_label,#I.#P:#{?@renamed,#W: ,}#{@pane_label},}",
        ])
        .map_err(anyhow::Error::msg)?,
    )?;
    let mut entries = windows
        .lines()
        .chain(panes.lines())
        .filter(|line| !line.is_empty())
        .map(Entry::parse)
        .collect::<Result<Vec<_>>>()?;
    entries.sort();

    let picked = picker::pick(&render(&entries), "> ", Matching::Fuzzy)?;
    if let Some(index) = picked.and_then(|picked| picked.item) {
        entries[index].select()?;
    }
    Ok(())
}

fn render(entries: &[Entry]) -> Vec<Line<'static>> {
    let windows = entries
        .iter()
        .map(|entry| entry.window.to_string())
        .collect::<Vec<_>>();
    let suffixes = entries
        .iter()
        .map(|entry| entry.pane.map_or(String::new(), |pane| format!(".{pane}")))
        .collect::<Vec<_>>();
    let window_width = windows.iter().map(String::len).max().unwrap_or(0);
    let suffix_width = suffixes.iter().map(String::len).max().unwrap_or(0);
    entries
        .iter()
        .zip(windows.iter().zip(&suffixes))
        .map(|(entry, (window, suffix))| {
            let mut line =
                Line::from(format!("{window:>window_width$}{suffix:<suffix_width$}").bold());
            if let Some(label) = &entry.label {
                line.push_span(format!("  {label}"));
            }
            line
        })
        .collect()
}
