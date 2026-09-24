use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand};

#[derive(ClapArgs)]
#[command(
    about = "Record Matroska track info in a mkv_info.db and list files with unexpected languages"
)]
pub struct MkvInfoRecorder {
    /// Database path (skips discovery)
    #[arg(short, long, global = true, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Create the database if one does not already exist
    #[arg(long, global = true)]
    pub mkdb: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Record new and changed files, then list those with languages other than eng or overrides
    Check {
        #[arg(value_name = "PATH", default_value = ".")]
        paths: Vec<PathBuf>,
    },
    /// List recorded files under PATH with their audio and subtitle languages
    List {
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,
        /// Print JSON instead of one line per file
        #[arg(long)]
        json: bool,
        /// Only files with an audio or subtitle track in LANGUAGE (repeatable; any match)
        #[arg(short, long = "language", value_name = "LANGUAGE")]
        languages: Vec<String>,
        /// Print absolute paths instead of paths relative to the database directory
        #[arg(short, long)]
        absolute: bool,
        /// Only files whose printed path (relative, or absolute with --absolute) contains TEXT
        #[arg(short = 'm', long = "match", value_name = "TEXT")]
        pattern: Option<String>,
        /// Only files with languages other than eng or overrides
        #[arg(short, long)]
        unexpected: bool,
    },
    /// Manage the languages allowed under a path
    Override {
        #[command(subcommand)]
        command: OverrideCommand,
    },
}

#[derive(Subcommand)]
pub enum OverrideCommand {
    /// List overrides under PATH
    List {
        #[arg(value_name = "PATH", default_value = ".")]
        path: PathBuf,
    },
    /// Allow languages under PATH
    Add {
        #[arg(value_name = "PATH")]
        path: PathBuf,
        #[arg(value_name = "LANGUAGE", required = true)]
        languages: Vec<String>,
    },
    /// Disallow languages under PATH, or all of them when none are given
    Rm {
        #[arg(value_name = "PATH")]
        path: PathBuf,
        #[arg(value_name = "LANGUAGE")]
        languages: Vec<String>,
    },
}
