use std::path::PathBuf;

use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Download urls from arguments, stdin, or the clipboard")]
pub struct Dl {
    #[arg(short, long, value_name = "FILE", help = "name of the downloaded file")]
    pub output: Option<PathBuf>,
    #[arg(long, help = "skip tls certificate verification")]
    pub insecure: bool,
    #[arg(long, help = "download every url with yt-dlp")]
    pub yt: bool,
    #[arg(value_name = "URL")]
    pub urls: Vec<String>,
}
