mod preview;
mod profile;
mod repo;

use std::env;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Args as ClapArgs;

use crate::dirs;
use crate::tools::Tool;
use crate::tools::base16_shell_preview::profile::Profile;
use crate::tools::base16_shell_preview::repo::{Order, Repo};

const KEYS: &str = "\
Keys:
  up/down      move 1
  pgup/pgdown  move page
  home/end     go to beginning/end
  q            quit
  enter        enable theme and quit";

#[derive(ClapArgs)]
#[command(
    about = "Preview Base16 Shell themes in the terminal and enable one",
    after_help = KEYS
)]
pub struct Base16ShellPreview {
    #[arg(long, help = "Sort themes by background, darkest to lightest")]
    sort_bg: bool,

    #[arg(
        short,
        long,
        conflicts_with = "theme",
        help = "Print the theme names and exit"
    )]
    list: bool,

    #[arg(help = "Enable this theme and exit")]
    theme: Option<String>,
}

impl Tool for Base16ShellPreview {
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

fn run(args: Base16ShellPreview) -> Result<()> {
    let profile = Profile::new(&dirs::home()?, env::var_os("BASE16_SHELL_HOOKS"));
    let repo = Repo::locate(env::var_os("BASE16_SHELL").as_deref(), &profile)?;
    let order = if args.sort_bg {
        Order::Background
    } else {
        Order::Name
    };
    let themes = repo.themes(order)?;

    if let Some(name) = args.theme {
        let theme = themes
            .iter()
            .find(|theme| theme.name() == name)
            .with_context(|| format!("theme not found: {name}"))?;
        theme.apply()?;
        return profile.enable(theme);
    }
    if args.list {
        for theme in &themes {
            println!("{}", theme.name());
        }
        return Ok(());
    }
    preview::run(&themes, &profile)
}
