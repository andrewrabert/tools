use std::path::PathBuf;

use clap::{Args as ClapArgs, Subcommand, ValueEnum};

#[derive(ClapArgs)]
#[command(
    about = "Manage Claude Code settings, MCP servers, marketplaces and plugins from a spec directory"
)]
pub struct ClaudeConfig {
    #[arg(
        short,
        long,
        global = true,
        help = "only print warnings, errors and the summary"
    )]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(about = "reconcile the installed state to match a spec directory")]
    Apply {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
    },
    #[command(about = "write the spec of the current installed state to a directory")]
    Export {
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        #[arg(short, long, help = "overwrite a spec that already exists")]
        force: bool,
        #[arg(short = 'n', long, help = "show what would change without writing")]
        dry_run: bool,
    },
    #[command(about = "print a commented mcp, marketplace, or plugin spec template")]
    New {
        #[arg(value_enum)]
        kind: Template,
    },
    #[command(about = "validate spec file syntax")]
    Validate {
        #[arg(help = "spec file, or a directory holding claude-config.yaml; stdin when omitted")]
        spec: Option<PathBuf>,
    },
    #[command(about = "update all marketplaces and installed plugins")]
    Update,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Template {
    Mcp,
    Marketplace,
    Plugin,
}

impl Template {
    pub fn text(self) -> &'static str {
        match self {
            Template::Mcp => MCP_TEMPLATE,
            Template::Marketplace => MARKETPLACE_TEMPLATE,
            Template::Plugin => PLUGIN_TEMPLATE,
        }
    }
}

const MCP_TEMPLATE: &str = "\
# mcp server spec - uncomment one section
# stdio server
# type: stdio
# command:
# args: []
# env: {}
# enabled:

# remote server
# type: http  # http | sse
# url:
# headers: {}
# headersHelper:
# enabled:
";

const MARKETPLACE_TEMPLATE: &str = "\
# marketplace spec - name comes from the filename
# type: github  # github | git | directory | local | url | npm | git-subdir
# source:
# enabled: []
# disabled: []
# available: []
";

const PLUGIN_TEMPLATE: &str = "\
# plugin spec - name comes from the filename and must match the repo's plugin.json name; uncomment one section
# github (source: owner/repo)
# type: github
# source:
# ref:
# sha:
# enabled:

# git (source: git url)
# type: git
# source:
# ref:
# sha:
# enabled:

# git-subdir (source: git url, path: subdir)
# type: git-subdir
# source:
# path:
# ref:
# sha:
# enabled:

# npm (source: package name)
# type: npm
# source:
# version:
# registry:
# enabled:
";
