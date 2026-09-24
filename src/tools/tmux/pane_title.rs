use std::process::ExitCode;

use anyhow::Result;
use clap::Args as ClapArgs;

use crate::tmux;
use crate::tools::Tool;
use crate::tools::tmux::exit_code;

#[derive(ClapArgs)]
#[command(about = "Set or clear the label of a tmux pane")]
pub struct PaneTitle {
    #[arg(value_name = "PANE_ID")]
    pub pane: String,
    #[arg(value_name = "TITLE", help = "clears the label when empty or absent")]
    pub title: Option<String>,
}

impl Tool for PaneTitle {
    fn run(self) -> ExitCode {
        exit_code(run(self))
    }
}

fn run(args: PaneTitle) -> Result<()> {
    let pane = args.pane.as_str();
    let window = String::from_utf8(
        tmux::capture(["display-message", "-t", pane, "-p", "#{window_id}"])
            .map_err(anyhow::Error::msg)?,
    )?;
    let window = window.trim_end();
    match args.title.filter(|title| !title.is_empty()) {
        Some(title) => {
            tmux::run(["set-option", "-p", "-t", pane, "@pane_label", &title])
                .map_err(anyhow::Error::msg)?;
            tmux::run([
                "set-option",
                "-w",
                "-t",
                window,
                "pane-border-status",
                "top",
            ])
            .map_err(anyhow::Error::msg)?;
            Ok(())
        }
        None => {
            tmux::run(["set-option", "-up", "-t", pane, "@pane_label"])
                .map_err(anyhow::Error::msg)?;
            let labels = String::from_utf8(
                tmux::capture(["list-panes", "-t", window, "-F", "#{@pane_label}"])
                    .map_err(anyhow::Error::msg)?,
            )?;
            if labels.lines().all(str::is_empty) {
                tmux::run([
                    "set-option",
                    "-w",
                    "-t",
                    window,
                    "pane-border-status",
                    "off",
                ])
                .map_err(anyhow::Error::msg)?;
            }
            Ok(())
        }
    }
}
