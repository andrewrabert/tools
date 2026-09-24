mod clipboard;
mod dispatch;
mod input;
mod source;
mod tools;

use std::env;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::builder::Styles;
use clap::{Command, Parser, Subcommand};

use crate::tools::Tool;

pub const APP_NAME: &str = env!("CARGO_CRATE_NAME");
pub const APP_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), env!("VERSION_SUFFIX"));

/// A section of related tools in the help.
struct Group {
    heading: &'static str,
    description: Option<&'static str>,
    names: &'static [&'static str],
}

macro_rules! tool_groups {
    (@description) => { None };
    (@description $description:literal) => { Some($description) };
    ($tools:ident, $aliases:path; $($group:literal $(: $description:literal)? {
        $($name:literal => $module:ident :: $tool:ident),* $(,)?
    })*) => {
        #[derive(Subcommand)]
        #[command(display_name = APP_NAME)]
        enum $tools {
            $($(
                #[command(name = $name, aliases = $aliases($name))]
                $tool(tools::$module::$tool),
            )*)*
        }

        impl $tools {
            const GROUPS: &[Group] = &[
                $(Group {
                    heading: $group,
                    description: tool_groups!(@description $($description)?),
                    names: &[$($name),*],
                }),*
            ];

            fn run(self) -> ExitCode {
                match self {
                    $($($tools::$tool(tool) => tool.run(),)*)*
                }
            }
        }
    };
}

/// Tools that also run under their own name.
macro_rules! tools {
    ($($groups:tt)*) => {
        tool_groups! { Tools, standalone_aliases; $($groups)* }
    };
}

/// Tools that only run through the binary itself, never under their own name.
macro_rules! internal_tools {
    ($($groups:tt)*) => {
        tool_groups! { InternalTools, no_aliases; $($groups)* }
    };
}

fn standalone_aliases(name: &str) -> Vec<String> {
    vec![format!("{APP_NAME}-{name}")]
}

fn no_aliases(_name: &str) -> Vec<String> {
    Vec::new()
}

tools! {
    "Archive": "Create and unpack archives" {
        "archive" => archive::Archive,
        "extract" => archive::Extract,
    }
    "Clipboard" {
        "cbcopy" => cb::Cbcopy,
        "cbpaste" => cb::Cbpaste,
    }
    "Encoding" {
        "b64d" => b64::B64d,
        "b64e" => b64::B64e,
        "binary2hex" => radix::Binary2hex,
        "encode-utf8" => encode_utf8::EncodeUtf8,
        "hex2binary" => radix::Hex2binary,
        "urldecode" => url::Urldecode,
        "urlencode" => url::Urlencode,
        "zlib" => zlib::Zlib,
    }
    "Files" {
        "code-nuke" => code_nuke::CodeNuke,
        "dotfiles-link-bin" => dotfiles_link_bin::DotfilesLinkBin,
        "empty-tree" => empty_tree::EmptyTree,
        "escape-files-windows" => escape_files_windows::EscapeFilesWindows,
        "file-hash-recorder" => file_hash_recorder::FileHashRecorder,
        "find-dupes" => find_dupes::FindDupes,
        "find-file-exts" => find_file_exts::FindFileExts,
        "mkv-preview" => mkv_preview::MkvPreview,
        "path-renamer" => path_renamer::PathRenamer,
        "pathbin" => pathbin::Pathbin,
        "pathcacher" => pathcacher::Pathcacher,
        "preview" => preview::Preview,
        "rerename" => rerename::Rerename,
    }
    "JSON" {
        "json-flat" => json::JsonFlat,
        "json-nest" => json::JsonNest,
        "json-re" => json::JsonRe,
    }
    "Media" {
        "img-compare" => img::Compare,
        "img-convert" => img::Convert,
        "img-fixcolorspace" => img::Fixcolorspace,
        "img-metadata" => img::Metadata,
        "img-optim" => img::Optim,
        "mkv-set-title-from-filename" => mkv_set_title_from_filename::MkvSetTitleFromFilename,
        "music-organizer" => music_organizer::MusicOrganizer,
        "reencode-lossless" => reencode_lossless::ReencodeLossless,
    }
    "Network" {
        "dl" => dl::Dl,
        "geoip" => geoip::Geoip,
        "mac-address" => mac_address::MacAddress,
        "publicipv4" => publicip::PublicIpv4,
        "publicipv6" => publicip::PublicIpv6,
    }
    "System" {
        "fanpipe" => fanpipe::Fanpipe,
        "full-update" => full_update::FullUpdate,
        "lmk" => lmk::Lmk,
    }
    "Text" {
        "txt-alternating" => txt::Alternating,
        "txt-lower" => txt::Lower,
        "txt-trim" => txt::Trim,
        "txt-trimlines" => txt::Trimlines,
        "txt-upper" => txt::Upper,
        "txt-upper-first" => txt::UpperFirst,
        "urlextract" => urlextract::Urlextract,
    }
}

internal_tools! {
    "Bertbox": "Set up and inspect bertbox itself" {
        "completions" => bertbox::Completions,
        "install" => bertbox::Install,
        "list" => bertbox::List,
    }
}

/// Every tool the binary itself answers to.
#[derive(Subcommand)]
enum AnyTool {
    #[command(flatten)]
    Tool(Tools),
    #[command(flatten)]
    Internal(InternalTools),
}

impl AnyTool {
    fn groups() -> impl Iterator<Item = &'static Group> {
        Tools::GROUPS.iter().chain(InternalTools::GROUPS)
    }

    fn run(self) -> ExitCode {
        match self {
            AnyTool::Tool(tool) => tool.run(),
            AnyTool::Internal(tool) => tool.run(),
        }
    }
}

/// Help with one section per tool group in place of clap's single tool list.
fn help_template() -> String {
    let tools = AnyTool::augment_subcommands(Command::new(APP_NAME));
    let styles = Styles::default();
    let header = styles.get_header();
    let literal = styles.get_literal();
    let width = AnyTool::groups()
        .flat_map(|group| group.names)
        .map(|name| name.len())
        .max()
        .unwrap_or(0);

    let mut template = String::from("{usage-heading} {usage}\n\n");
    for group in AnyTool::groups() {
        template.push_str(&format!("{header}{}:{header:#}\n", group.heading));
        if let Some(description) = group.description {
            template.push_str(&format!("  {description}\n\n"));
        }
        for name in group.names {
            let about = tools
                .find_subcommand(name)
                .and_then(Command::get_about)
                .map(ToString::to_string)
                .unwrap_or_default();
            template.push_str(&format!("  {literal}{name:width$}{literal:#}  {about}\n"));
        }
        template.push('\n');
    }
    template.push_str(&format!("{header}Options:{header:#}\n{{options}}"));
    template
}

#[derive(Parser)]
#[command(
    multicall = true,
    name = APP_NAME,
    version = APP_VERSION,
    propagate_version = true,
    disable_help_subcommand = true
)]
enum Cli {
    #[command(
        name = APP_NAME,
        version = APP_VERSION,
        disable_help_subcommand = true,
        subcommand_value_name = "TOOL",
        help_template = help_template()
    )]
    Bertbox(Bertbox),
    #[command(flatten)]
    Tool(Tools),
}

#[derive(Parser)]
#[command(
    name = APP_NAME,
    version = APP_VERSION,
    propagate_version = true,
    disable_help_subcommand = true,
    subcommand_value_name = "TOOL",
    help_template = help_template()
)]
struct Bertbox {
    #[command(subcommand)]
    tool: AnyTool,
}

pub fn take_as_bertbox() -> Result<bool> {
    let Some(value) = env::var_os(dispatch::ENV_NAME) else {
        return Ok(false);
    };
    // SAFETY: called first thing in main, before any other thread exists.
    unsafe { env::remove_var(dispatch::ENV_NAME) };
    if value != dispatch::ENV_VALUE {
        bail!(
            "{} must be {:?}, got {value:?}",
            dispatch::ENV_NAME,
            dispatch::ENV_VALUE
        );
    }
    Ok(true)
}

fn main() -> ExitCode {
    let as_bertbox = match take_as_bertbox() {
        Ok(as_bertbox) => as_bertbox,
        Err(error) => {
            eprintln!("Error: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    if as_bertbox {
        return Bertbox::parse().tool.run();
    }
    match Cli::parse() {
        Cli::Bertbox(Bertbox { tool }) => tool.run(),
        Cli::Tool(tool) => tool.run(),
    }
}
