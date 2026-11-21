//! A port of lnkedit by Iain Patterson (<http://iain.cx/src/lnkedit/>), reading and writing
//! shortcuts directly instead of through IShellLink.
mod lnk;

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use regex::RegexBuilder;

use crate::atomic_write;
use crate::tools::Tool;
use crate::tools::lnkedit::lnk::ShellLink;

#[derive(ClapArgs)]
#[command(
    about = "Show or retarget a Windows shortcut (.lnk)",
    long_about = "Show the target, arguments, and working directory of a Windows shortcut. With \
                  REGEX, show them only when one matches. With REPLACEMENT too, rewrite every \
                  match in the chosen fields (all three by default) and save the shortcut. \
                  REGEX is case-insensitive, as Windows paths are; REPLACEMENT may refer to \
                  groups as $1 or ${name}.",
    after_help = "Example:\n  lnkedit -td app.lnk 'C:\\Program Files' '%programfiles%'\n\n\
                  Replaces C:\\Program Files in the target and working directory, leaving the \
                  arguments alone."
)]
pub struct Lnkedit {
    /// Match the target
    #[arg(short, long)]
    target: bool,

    /// Match the arguments
    #[arg(short, long)]
    arguments: bool,

    /// Match the working directory
    #[arg(short = 'd', long)]
    working_dir: bool,

    #[arg(value_name = "FILE")]
    file: PathBuf,

    #[arg(value_name = "REGEX")]
    regex: Option<String>,

    #[arg(value_name = "REPLACEMENT", requires = "regex")]
    replacement: Option<String>,
}

impl Tool for Lnkedit {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:#}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Lnkedit) -> Result<()> {
    let all = !(args.target || args.arguments || args.working_dir);
    let data = fs::read(&args.file).with_context(|| format!("reading {}", args.file.display()))?;
    let mut link = ShellLink::parse(&data).with_context(|| args.file.display().to_string())?;
    let regex = args
        .regex
        .as_deref()
        .map(|pattern| {
            RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .with_context(|| format!("parsing {pattern:?}"))
        })
        .transpose()?;

    let mut target = link.target().unwrap_or_default();
    let mut arguments = link.arguments().unwrap_or_default().to_owned();
    let mut working_dir = link.working_dir().unwrap_or_default().to_owned();
    let mut matched = regex.is_none();
    let mut changed = false;
    if let Some(regex) = &regex {
        let mut field = |on: bool, value: &mut String| -> Result<bool> {
            if !(on || all) || !regex.is_match(value) {
                return Ok(false);
            }
            matched = true;
            if let Some(replacement) = &args.replacement {
                let replaced = regex.replace_all(value, replacement.as_str()).into_owned();
                if replaced != *value {
                    *value = replaced;
                    return Ok(true);
                }
            }
            Ok(false)
        };
        if field(args.target, &mut target)? {
            link.set_target(&target)?;
            changed = true;
        }
        if field(args.arguments, &mut arguments)? {
            link.set_arguments(&arguments);
            changed = true;
        }
        if field(args.working_dir, &mut working_dir)? {
            link.set_working_dir(&working_dir);
            changed = true;
        }
    }

    print!(" Shortcut: {}", args.file.display());
    if matched {
        println!();
        println!("   Target: {target}");
        println!("Arguments: {arguments}");
        println!(" Start in: {working_dir}");
    } else {
        println!(" (no match)");
    }

    if changed {
        let bytes = link.to_bytes()?;
        let temp = atomic_write::sibling(".lnkedit-", &args.file)?;
        fs::File::create(&temp)
            .and_then(|mut file| file.write_all(&bytes))
            .with_context(|| format!("writing {}", temp.display()))?;
        atomic_write::replace(temp, &args.file)?;
    }
    Ok(())
}
