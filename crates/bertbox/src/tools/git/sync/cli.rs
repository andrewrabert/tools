use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Clone, fetch, and track source repositories under ~/src")]
pub struct Sync {
    #[arg(long, group = "cache", help = "print the tracked repositories")]
    pub show_cache: bool,
    #[arg(long, group = "cache", help = "print the tracked backup repositories")]
    pub show_backup_cache: bool,
    #[arg(
        long,
        group = "cache",
        help = "rebuild the caches from the repositories on disk"
    )]
    pub build_cache: bool,
    #[arg(long, help = "delete empty directories that hold no repository")]
    pub clean_empty: bool,
    #[arg(
        long,
        help = "print what --clean-empty would delete without deleting it"
    )]
    pub dry_run: bool,
    #[arg(long, help = "list known git hosts")]
    pub list_hosts: bool,
    #[arg(long, help = "print the host:path a repository resolves to")]
    pub parse_url: bool,
    #[arg(long, help = "delete the repository")]
    pub rm: bool,
    #[arg(value_name = "REPO")]
    pub repo: Option<String>,
}
