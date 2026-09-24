use clap::Args as ClapArgs;

#[derive(ClapArgs)]
#[command(about = "Link the scripts of every dotfiles directory into the dotfiles bin")]
pub struct DotfilesLinkBin {}
