mod api;
mod cli;
mod repo;

use std::io::{self, IsTerminal, Write};
use std::num::NonZeroUsize;
use std::process::ExitCode;

use anyhow::{Result, bail};
use crossterm::style::{Color, Stylize};
use serde_json::{Value, json};

use crate::tools::Tool;
use crate::tools::forgejo::api::Api;
pub use crate::tools::forgejo::cli::Forgejo;
use crate::tools::forgejo::cli::{Command, Output};
use crate::tools::forgejo::repo::{Repo, RepoName, Visibility, unique};

impl Tool for Forgejo {
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

fn run(args: Forgejo) -> Result<()> {
    let api = Api::new(&args.url, &args.token, args.output.verbose)?;
    let output = &args.output;
    let mut out = io::stdout().lock();
    let command = args.command.unwrap_or(Command::List {
        owner: None,
        limit: None,
    });
    match command {
        Command::List { owner, limit } => list(&api, output, owner.as_deref(), limit, &mut out)?,
        Command::Info { repos } => info(&api, output, repos, &mut out)?,
        Command::Create {
            repo,
            public,
            description,
        } => {
            let created = api.create_repo(&repo, !public, &description)?;
            if output.print_name {
                writeln!(out, "{}", created.full_name)?;
            } else if io::stdout().is_terminal() {
                let visibility = Visibility::of(created.private).as_str();
                writeln!(out, "{} ({visibility})", created.full_name)?;
                if let Some(description) = created.description.filter(|d| !d.is_empty()) {
                    writeln!(out, "{description}")?;
                }
                writeln!(out, "{}", created.html_url)?;
            } else {
                writeln!(out, "{}", created.html_url)?;
            }
        }
        Command::Delete { repos } => {
            for repo in unique(repos) {
                let deleted = api.delete_repo(&repo)?;
                if output.print_name {
                    writeln!(out, "{repo}")?;
                } else if deleted {
                    writeln!(out, "deleted {repo}")?;
                } else {
                    writeln!(out, "not found {repo}")?;
                }
            }
        }
        Command::Rename { repo, new_name } => {
            let renamed = api.edit_repo(&repo, &json!({ "name": new_name }))?;
            writeln!(out, "{}", renamed.full_name)?;
        }
        Command::Archive { repos } => edit_each(
            &api,
            output,
            repos,
            &json!({ "archived": true }),
            |name| format!("archived {name}"),
            &mut out,
        )?,
        Command::Unarchive { repos } => edit_each(
            &api,
            output,
            repos,
            &json!({ "archived": false }),
            |name| format!("unarchived {name}"),
            &mut out,
        )?,
        Command::Public { repos } => edit_each(
            &api,
            output,
            repos,
            &json!({ "private": false }),
            |name| format!("{name} is now public"),
            &mut out,
        )?,
        Command::Private { repos } => edit_each(
            &api,
            output,
            repos,
            &json!({ "private": true }),
            |name| format!("{name} is now private"),
            &mut out,
        )?,
        Command::Describe { repo, description } => {
            let described = api.edit_repo(&repo, &json!({ "description": description }))?;
            if output.print_name {
                writeln!(out, "{}", described.full_name)?;
            } else {
                let description = described.description.unwrap_or_default();
                writeln!(out, "{}: {description}", described.full_name)?;
            }
        }
    }
    out.flush()?;
    Ok(())
}

fn list(
    api: &Api,
    output: &Output,
    owner: Option<&str>,
    limit: Option<NonZeroUsize>,
    out: &mut impl Write,
) -> Result<()> {
    if output.print_name {
        bail!("--print-name requires owner/repo");
    }
    let mut repos: Vec<Repo> = api
        .list_repos(owner, limit)?
        .into_iter()
        .map(Repo::from)
        .collect();
    repos.sort_by(|a, b| (a.visibility, &a.name).cmp(&(b.visibility, &b.name)));
    if output.json {
        return write_json(&repos, out);
    }
    let name_width = repos
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let visibility_width = repos
        .iter()
        .map(|r| r.visibility.as_str().len())
        .max()
        .unwrap_or(0);
    for repo in &repos {
        let color = match repo.visibility {
            Visibility::Public => Color::DarkGreen,
            Visibility::Private => Color::DarkYellow,
        };
        writeln!(
            out,
            "{:<name_width$}  {}  {}",
            repo.name,
            format!("{:<visibility_width$}", repo.visibility.as_str()).with(color),
            repo.url
        )?;
    }
    Ok(())
}

fn info(api: &Api, output: &Output, repos: Vec<RepoName>, out: &mut impl Write) -> Result<()> {
    let infos = unique(repos)
        .iter()
        .map(|repo| api.get_repo(repo).map(Repo::from))
        .collect::<Result<Vec<_>>>()?;
    if output.print_name {
        for info in &infos {
            writeln!(out, "{}", info.name)?;
        }
    } else if output.json {
        write_json(&infos, out)?;
    } else if io::stdout().is_terminal() {
        for (index, info) in infos.iter().enumerate() {
            if index > 0 {
                writeln!(out)?;
            }
            info.write_fields(out)?;
        }
    } else {
        for info in &infos {
            writeln!(out, "{}", info.url)?;
        }
    }
    Ok(())
}

fn edit_each(
    api: &Api,
    output: &Output,
    repos: Vec<RepoName>,
    change: &Value,
    done: impl Fn(&str) -> String,
    out: &mut impl Write,
) -> Result<()> {
    for repo in unique(repos) {
        let edited = api.edit_repo(&repo, change)?;
        if output.print_name {
            writeln!(out, "{}", edited.full_name)?;
        } else {
            writeln!(out, "{}", done(&edited.full_name))?;
        }
    }
    Ok(())
}

fn write_json(repos: &[Repo], out: &mut impl Write) -> Result<()> {
    // Going through Value sorts the keys, matching the original's sort_keys.
    let value = serde_json::to_value(repos)?;
    serde_json::to_writer_pretty(&mut *out, &value)?;
    writeln!(out)?;
    Ok(())
}
