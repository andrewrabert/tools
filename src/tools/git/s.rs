use std::process::{ExitCode, Stdio};

use anyhow::Result;
use clap::Args as ClapArgs;
use crossterm::style::Stylize;

use crate::git::{self, Git};
use crate::tools::Tool;
use crate::tools::git::exit_code;

#[derive(ClapArgs)]
#[command(about = "Summarize remotes, branches, stashes, and changes")]
pub struct S {}

impl Tool for S {
    fn run(self) -> ExitCode {
        exit_code(run())
    }
}

fn run() -> Result<()> {
    let git = Git::cwd();
    git::run(git.command(["status"]).stdout(Stdio::null()))?;

    heading("Remote");
    let mut rows = Vec::new();
    for remote in git.capture(["remote"])?.lines() {
        let fetch = git
            .capture(["remote", "get-url", remote])?
            .trim_end()
            .to_owned();
        let push = git
            .capture(["remote", "get-url", "--push", remote])?
            .trim_end()
            .to_owned();
        if fetch == push {
            rows.push(vec![remote.to_owned(), fetch]);
        } else {
            rows.push(vec![remote.to_owned(), fetch, "(fetch)".to_owned()]);
            rows.push(vec![remote.to_owned(), push, "(push)".to_owned()]);
        }
    }
    print_columns(&rows);

    println!();
    heading("All Branch");
    git.run(["branch", "-vv"])?;

    let stash = git.capture(["stash", "list"])?;
    let stash = stash.trim_end_matches('\n');
    if !stash.is_empty() {
        println!();
        heading("Stash");
        println!("{stash}");
    }

    let status = git.capture(["-c", "color.status=always", "status", "--short"])?;
    let status = status.trim_end_matches('\n');
    if !status.is_empty() {
        println!();
        heading("Changes");
        println!("{status}");
    }
    Ok(())
}

fn heading(title: &str) {
    println!("{}", format!("> {title}").dark_yellow().bold());
}

fn print_columns(rows: &[Vec<String>]) {
    let mut widths = Vec::new();
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            match widths.get_mut(column) {
                Some(width) => *width = cell.chars().count().max(*width),
                None => widths.push(cell.chars().count()),
            }
        }
    }
    for row in rows {
        let last = row.len() - 1;
        let line: String = row
            .iter()
            .enumerate()
            .map(|(column, cell)| {
                if column == last {
                    cell.clone()
                } else {
                    format!("{cell:width$}  ", width = widths[column])
                }
            })
            .collect();
        println!("{line}");
    }
}
