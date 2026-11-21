mod apply;
mod cli;
mod dump;
mod field;
mod json;
mod tags;
mod tui;

use std::process::ExitCode;

use anyhow::{Result, bail};

use crate::tools::Tool;
use crate::tools::audio_tag::apply::Mode;
pub use crate::tools::audio_tag::cli::AudioTag;
use crate::tools::audio_tag::json::{FileUpdates, Update};

impl Tool for AudioTag {
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

fn run(args: AudioTag) -> Result<()> {
    if args.tui {
        let [path] = args.paths.as_slice() else {
            bail!("TUI mode requires exactly one path");
        };
        return tui::run(path);
    }
    if !args.set.is_empty() || !args.delete.is_empty() {
        if args.paths.is_empty() {
            bail!("set mode requires file paths");
        }
        let mut updates: Vec<(String, Update)> = Vec::new();
        let sets = args
            .set
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[name, value]| (name, Update::Set(vec![value.clone()])));
        let deletes = args.delete.iter().map(|name| (name, Update::Delete));
        for (name, update) in sets.chain(deletes) {
            let name = name.to_lowercase();
            updates.retain(|(existing, _)| *existing != name);
            updates.push((name, update));
        }
        let files: Vec<FileUpdates> = args
            .paths
            .into_iter()
            .map(|path| FileUpdates {
                path,
                updates: updates.clone(),
            })
            .collect();
        return apply::apply(&files, Mode::Set);
    }
    if let Some(input) = &args.input {
        let files = json::read(input, args.structured)?;
        let mode = if args.merge {
            Mode::Merge
        } else {
            Mode::Replace
        };
        return apply::apply(&files, mode);
    }
    let [path] = args.paths.as_slice() else {
        bail!("dump mode requires exactly one path");
    };
    dump::dump(path, args.structured, args.output.as_deref())
}
