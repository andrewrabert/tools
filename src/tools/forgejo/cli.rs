use std::num::NonZeroUsize;

use clap::{Args as ClapArgs, Subcommand};

use crate::tools::forgejo::repo::RepoName;

#[derive(ClapArgs)]
#[command(about = "Manage repositories on a Forgejo server")]
pub struct Forgejo {
    #[arg(long, env = "FORGEJO_URL", help = "base url of the server")]
    pub url: String,
    #[arg(
        long,
        env = "FORGEJO_TOKEN",
        hide_env_values = true,
        help = "api access token"
    )]
    pub token: String,
    #[command(flatten)]
    pub output: Output,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(ClapArgs)]
pub struct Output {
    #[arg(long, global = true, help = "output as JSON")]
    pub json: bool,
    #[arg(long, global = true, help = "output raw API responses")]
    pub verbose: bool,
    #[arg(long, global = true, help = "only output owner/repo")]
    pub print_name: bool,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "list repos")]
    List {
        #[arg(help = "filter by owner")]
        owner: Option<String>,
        #[arg(
            long,
            value_name = "N",
            help = "stop after the first N repos the server returns"
        )]
        limit: Option<NonZeroUsize>,
    },
    #[command(about = "show repo info")]
    Info {
        #[arg(required = true, value_name = "OWNER/REPO")]
        repos: Vec<RepoName>,
    },
    #[command(about = "create repo")]
    Create {
        #[arg(value_name = "OWNER/REPO")]
        repo: RepoName,
        #[arg(long, help = "make public")]
        public: bool,
        #[arg(long, default_value = "", help = "repo description")]
        description: String,
    },
    #[command(about = "delete repo")]
    Delete {
        #[arg(required = true, value_name = "OWNER/REPO")]
        repos: Vec<RepoName>,
    },
    #[command(about = "rename repo")]
    Rename {
        #[arg(value_name = "OWNER/REPO")]
        repo: RepoName,
        new_name: String,
    },
    #[command(about = "archive repo")]
    Archive {
        #[arg(required = true, value_name = "OWNER/REPO")]
        repos: Vec<RepoName>,
    },
    #[command(about = "unarchive repo")]
    Unarchive {
        #[arg(required = true, value_name = "OWNER/REPO")]
        repos: Vec<RepoName>,
    },
    #[command(about = "make repo public")]
    Public {
        #[arg(required = true, value_name = "OWNER/REPO")]
        repos: Vec<RepoName>,
    },
    #[command(about = "make repo private")]
    Private {
        #[arg(required = true, value_name = "OWNER/REPO")]
        repos: Vec<RepoName>,
    },
    #[command(about = "set repo description")]
    Describe {
        #[arg(value_name = "OWNER/REPO")]
        repo: RepoName,
        description: String,
    },
}
