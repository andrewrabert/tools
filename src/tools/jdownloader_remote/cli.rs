use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand, ValueEnum};
use serde_json::Value;

use crate::tools::jdownloader_remote::model::{
    AccountId, ArchiveId, CaptchaId, CleanupAction, CleanupMode, LinkId, PackageId, Priority,
    VariantId,
};
use crate::tools::jdownloader_remote::output::Format;

#[derive(ClapArgs)]
#[command(about = "Control JDownloader through its local API")]
pub struct JdownloaderRemote {
    #[arg(long, env = "JDOWNLOADER_URL", help = "API base URL")]
    pub url: String,
    #[arg(
        long,
        env = "JDOWNLOADER_USERNAME",
        requires = "password",
        help = "API username"
    )]
    pub username: Option<String>,
    #[arg(
        long,
        env = "JDOWNLOADER_PASSWORD",
        hide_env_values = true,
        requires = "username",
        help = "API password"
    )]
    pub password: Option<String>,
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t = Format::Text,
        help = "Output format"
    )]
    pub format: Format,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Show JDownloader version")]
    Version,
    #[command(about = "Show API endpoint help")]
    ApiHelp,
    #[command(
        about = "Add container files and URLs; URLs start downloading",
        long_about = "Add container files and URLs; URLs start downloading.\n\n\
                      Each argument is a container file. \
                      The ftp, http, and https URLs in piped stdin are added."
    )]
    Add {
        #[arg(value_name = "PATH", help = "Container file to add")]
        files: Vec<PathBuf>,
    },
    #[command(about = "Clean up the download list")]
    Cleanup {
        #[arg(
            long,
            value_enum,
            default_value_t = CleanupAction::Finished,
            help = "Links to clean up"
        )]
        action: CleanupAction,
        #[arg(
            long,
            value_enum,
            default_value_t = CleanupMode::LinksOnly,
            help = "What to do with the links and their files"
        )]
        mode: CleanupMode,
        #[arg(
            long = "link",
            value_name = "ID",
            help = "Limit cleanup to a link (repeatable)"
        )]
        links: Vec<LinkId>,
        #[arg(
            long = "package",
            value_name = "ID",
            help = "Limit cleanup to a package (repeatable)"
        )]
        packages: Vec<PackageId>,
    },
    #[command(subcommand, about = "Manage the download list")]
    Downloads(Downloads),
    #[command(subcommand, about = "Manage the link grabber")]
    Grabber(Grabber),
    #[command(subcommand, about = "Manage pending captchas")]
    Captcha(Captcha),
    #[command(subcommand, about = "Manage hoster accounts")]
    Account(Account),
    #[command(subcommand, about = "Manage archive extraction")]
    Extract(Extract),
    #[command(subcommand, about = "Manage advanced settings")]
    Config(Config),
    #[command(subcommand, about = "Show hoster plugins")]
    Plugins(Plugins),
}

#[derive(ClapArgs)]
#[group(required = true, multiple = true)]
pub struct Selection {
    #[arg(long = "link", value_name = "ID", help = "Select a link (repeatable)")]
    pub links: Vec<LinkId>,
    #[arg(
        long = "package",
        value_name = "ID",
        help = "Select a package (repeatable)"
    )]
    pub packages: Vec<PackageId>,
}

#[derive(ClapArgs)]
#[group(required = true, multiple = false)]
pub struct RenameTarget {
    #[arg(long, value_name = "ID", help = "Select the link")]
    pub link: Option<LinkId>,
    #[arg(long, value_name = "ID", help = "Select the package")]
    pub package: Option<PackageId>,
}

#[derive(ClapArgs)]
pub struct Accounts {
    #[arg(required = true, value_name = "ACCOUNT_ID", help = "Account ID")]
    pub ids: Vec<AccountId>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Sort {
    Name,
    Status,
    Percent,
    Speed,
    Eta,
}

#[derive(Subcommand)]
pub enum Downloads {
    #[command(about = "Add URLs and start downloading")]
    AddLinks {
        #[arg(long, help = "Package name")]
        package: Option<String>,
        #[arg(long, help = "Download folder")]
        folder: Option<String>,
        #[arg(long, help = "Archive extraction password")]
        extract_password: Option<String>,
        #[arg(long, help = "Download password")]
        download_password: Option<String>,
        #[arg(long, help = "Add to the link grabber without starting")]
        no_start: bool,
        #[arg(required = true, help = "URL to download")]
        urls: Vec<String>,
    },
    #[command(about = "Add container files (dlc, crawljob, ...) to the link grabber")]
    AddContainer {
        #[arg(long, help = "Delete each file after it is added")]
        rm: bool,
        #[arg(required = true, help = "Container file to add")]
        files: Vec<PathBuf>,
    },
    #[command(about = "List download links")]
    List {
        #[arg(long, help = "Show only running links")]
        running: bool,
        #[arg(
            long,
            value_name = "TEXT",
            help = "Show only links whose status contains TEXT"
        )]
        status: Option<String>,
        #[arg(long, value_enum, default_value_t = Sort::Name, help = "Sort by field")]
        sort: Sort,
    },
    #[command(about = "List download packages")]
    Packages,
    #[command(about = "Start downloading")]
    Start,
    #[command(about = "Stop downloading")]
    Stop,
    #[command(about = "Pause downloading")]
    Pause,
    #[command(about = "Unpause downloading")]
    Unpause,
    #[command(about = "Remove links or packages from the download list")]
    Remove(Selection),
    #[command(about = "Reset links or packages to not downloaded")]
    Reset(Selection),
    #[command(about = "Resume failed links or packages")]
    Resume(Selection),
    #[command(about = "Enable links or packages for downloading")]
    Enable(Selection),
    #[command(about = "Disable links or packages for downloading")]
    Disable(Selection),
    #[command(about = "Rename a link or package")]
    Rename {
        #[command(flatten)]
        target: RenameTarget,
        #[arg(help = "New name")]
        name: String,
    },
    #[command(about = "Set the download folder of packages")]
    SetFolder {
        #[arg(
            long = "package",
            value_name = "ID",
            required = true,
            help = "Select a package (repeatable)"
        )]
        packages: Vec<PackageId>,
        #[arg(help = "Download folder")]
        dir: String,
    },
    #[command(about = "Set the download priority of links or packages")]
    Priority {
        #[command(flatten)]
        selection: Selection,
        #[arg(value_enum, help = "Priority")]
        priority: Priority,
    },
}

#[derive(Subcommand)]
pub enum Grabber {
    #[command(about = "List links in the link grabber")]
    List,
    #[command(about = "Move links or packages to the download list")]
    Confirm {
        #[command(flatten)]
        selection: Selection,
        #[arg(long, help = "Move without starting")]
        no_start: bool,
    },
    #[command(about = "Clear the link grabber")]
    Clear,
    #[command(about = "List the variants of a link")]
    Variants {
        #[arg(help = "Link ID")]
        link_id: LinkId,
    },
    #[command(about = "Set the variant of a link")]
    SetVariant {
        #[arg(help = "Link ID")]
        link_id: LinkId,
        #[arg(help = "Variant ID")]
        variant_id: VariantId,
    },
}

#[derive(Subcommand)]
pub enum Captcha {
    #[command(about = "List pending captchas")]
    List,
    #[command(about = "Save a captcha image")]
    Image {
        #[arg(long, help = "Output file")]
        output: Option<PathBuf>,
        #[arg(help = "Captcha ID")]
        captcha_id: CaptchaId,
    },
    #[command(about = "Answer a captcha")]
    Solve {
        #[arg(help = "Captcha ID")]
        captcha_id: CaptchaId,
        #[arg(help = "Captcha answer")]
        answer: String,
    },
    #[command(about = "Skip a captcha")]
    Skip {
        #[arg(help = "Captcha ID")]
        captcha_id: CaptchaId,
    },
}

#[derive(Subcommand)]
pub enum Account {
    #[command(about = "List hoster accounts")]
    List,
    #[command(about = "Add a hoster account")]
    Add {
        #[arg(long, help = "Read the password from stdin")]
        password_stdin: bool,
        #[arg(help = "Hoster domain")]
        hoster: String,
        #[arg(help = "Account username")]
        username: String,
    },
    #[command(about = "Remove hoster accounts")]
    Remove(Accounts),
    #[command(about = "Enable hoster accounts for downloading")]
    Enable(Accounts),
    #[command(about = "Disable hoster accounts for downloading")]
    Disable(Accounts),
    #[command(about = "Refresh hoster account status")]
    Refresh(Accounts),
}

#[derive(Subcommand)]
pub enum Extract {
    #[command(about = "List archives and their extraction status")]
    List,
    #[command(about = "Start extracting archives")]
    Start(Selection),
    #[command(about = "Set an archive's password")]
    SetPassword {
        #[arg(help = "Archive ID")]
        archive_id: ArchiveId,
        #[arg(help = "Archive password")]
        password: String,
    },
}

#[derive(Subcommand)]
pub enum Config {
    #[command(about = "List advanced settings")]
    List {
        #[arg(help = "Show only settings whose key matches PATTERN")]
        pattern: Option<String>,
    },
    #[command(about = "Show an advanced setting")]
    Get {
        #[arg(help = "Config interface name")]
        interface: String,
        #[arg(help = "Setting key")]
        key: String,
    },
    #[command(about = "Change an advanced setting")]
    Set {
        #[arg(help = "Config interface name")]
        interface: String,
        #[arg(help = "Setting key")]
        key: String,
        #[arg(value_parser = parse_json, help = "New value as JSON")]
        value: Value,
    },
}

#[derive(Subcommand)]
pub enum Plugins {
    #[command(about = "List hoster plugins")]
    List {
        #[arg(help = "Show only plugins whose name matches PATTERN")]
        pattern: Option<String>,
    },
    #[command(about = "Show a hoster plugin's settings")]
    Settings {
        #[arg(help = "Hoster domain")]
        hoster: String,
    },
}

fn parse_json(value: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(value)
}
