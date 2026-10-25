mod atomic_write;
mod chardet;
mod clipboard;
mod cue;
mod dirs;
mod dispatch;
mod env_file;
mod git;
mod http;
mod input;
mod mime;
mod mkvmerge;
mod picker;
mod pool;
mod source;
mod sqlite;
mod tmux;
mod tools;
mod urls;
mod walk;

use std::env;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::builder::Styles;
use clap::{Command, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::tools::Tool;

pub const APP_NAME: &str = env!("CARGO_CRATE_NAME");
pub const APP_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), env!("VERSION_SUFFIX"));

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

fn standalone_aliases(name: &str) -> Vec<String> {
    vec![format!("{APP_NAME}-{name}")]
}

fn no_aliases(_name: &str) -> Vec<String> {
    Vec::new()
}

// Tools that also run under their own name.
tool_groups! {
    Tools, standalone_aliases;
    "Archive": "Create and unpack archives" {
        "archive" => archive::Archive,
        "extract" => archive::Extract,
    }
    "Claude" {
        "claude-statusline" => claude_statusline::ClaudeStatusline,
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
        "hexd" => hexd::Hexd,
        "qr" => qr::Qr,
        "urldecode" => url::Urldecode,
        "urlencode" => url::Urlencode,
        "zlib" => zlib::Zlib,
    }
    "Files" {
        "3ds-rom-tool" => three_ds_rom_tool::ThreeDsRomTool,
        "broken-links" => broken_links::BrokenLinks,
        "case-collisions" => case_collisions::CaseCollisions,
        "code-nuke" => code_nuke::CodeNuke,
        "empty-tree" => empty_tree::EmptyTree,
        "escape-files-windows" => escape_files_windows::EscapeFilesWindows,
        "file-hash-recorder" => file_hash_recorder::FileHashRecorder,
        "find-dupes" => find_dupes::FindDupes,
        "find-file-exts" => find_file_exts::FindFileExts,
        "mime" => mime::Mime,
        "mime-ext" => mime_ext::MimeExt,
        "mkv-preview" => mkv_preview::MkvPreview,
        "multihash" => multihash::Multihash,
        "path-renamer" => path_renamer::PathRenamer,
        "pathbin" => pathbin::Pathbin,
        "pathcacher" => pathcacher::Pathcacher,
        "preview" => preview::Preview,
        "rerename" => rerename::Rerename,
        "syncthing-conflicts" => syncthing_conflicts::SyncthingConflicts,
        "syncthing-stversions" => syncthing_stversions::SyncthingStversions,
    }
    "Git" {
        "git-auto-update" => git::AutoUpdate,
        "git-check-tag-is-pushed" => git::CheckTagIsPushed,
        "git-find-repos" => git::FindRepos,
        "git-remote-tags" => git::RemoteTags,
        "git-retime" => git::Retime,
        "git-s" => git::S,
        "git-sync" => git::Sync,
        "git-tag-timestamp" => git::TagTimestamp,
        "git-undelete" => git::Undelete,
        "git-web" => git::Web,
    }
    "JSON" {
        "json-flat" => json::JsonFlat,
        "json-nest" => json::JsonNest,
        "json-re" => json::JsonRe,
    }
    "Just" {
        "lust" => lust::Lust,
        "lust-manage" => lust::LustManage,
    }
    "Media" {
        "audio-tag" => audio_tag::AudioTag,
        "cue-tool" => cue_tool::CueTool,
        "img-compare" => img::Compare,
        "img-convert" => img::Convert,
        "img-fixcolorspace" => img::Fixcolorspace,
        "img-metadata" => img::Metadata,
        "img-optim" => img::Optim,
        "img-size" => img_size::ImgSize,
        "mkv-convert" => mkv_convert::MkvConvert,
        "mkv-extract-languages" => mkv_extract_languages::MkvExtractLanguages,
        "mkv-find-lang" => mkv_find_lang::MkvFindLang,
        "mkv-info-recorder" => mkv_info_recorder::MkvInfoRecorder,
        "mkv-language-strip" => mkv_language_strip::MkvLanguageStrip,
        "mkv-set-title-from-filename" => mkv_set_title_from_filename::MkvSetTitleFromFilename,
        "music-organizer" => music_organizer::MusicOrganizer,
        "reencode-lossless" => reencode_lossless::ReencodeLossless,
    }
    "Network" {
        "dl" => dl::Dl,
        "forgejo" => forgejo::Forgejo,
        "geoip" => geoip::Geoip,
        "immich-remote" => immich_remote::ImmichRemote,
        "jdownloader-remote" => jdownloader_remote::JdownloaderRemote,
        "mac-address" => mac_address::MacAddress,
        "network-scan" => network_scan::NetworkScan,
        "publicipv4" => publicip::PublicIpv4,
        "publicipv6" => publicip::PublicIpv6,
    }
    "System" {
        "cdemu-tool" => cdemu_tool::CdemuTool,
        "exit-code" => exit_code::ExitCodeTool,
        "fanpipe" => fanpipe::Fanpipe,
        "lmk" => lmk::Lmk,
        "open" => open::Open,
        "pending-sync" => pending_sync::PendingSync,
        "ud" => ud::Ud,
    }
    "Tmux" {
        "tmux-attach" => tmux::Attach,
        "tmux-attach-ssh" => tmux::AttachSsh,
        "tmux-chooser" => tmux::Chooser,
        "tmux-pane-title" => tmux::PaneTitle,
        "tmux-ssh" => tmux::Ssh,
    }
    "Text" {
        "txt-alternating" => txt::Alternating,
        "txt-line-end" => txt::LineEnd,
        "txt-lower" => txt::Lower,
        "txt-trim" => txt::Trim,
        "txt-trimlines" => txt::Trimlines,
        "txt-upper" => txt::Upper,
        "txt-upper-first" => txt::UpperFirst,
        "urlextract" => urlextract::Urlextract,
    }
    "External": "Commands from upstream crates" {
        "ast-grep" => external::AstGrep,
        "dotenvy" => external::Dotenvy,
        "qrencode" => external::Qrencode,
        "uncomment" => external::Uncomment,
    }
}

// Tools that only run through the binary itself, never under their own name.
tool_groups! {
    InternalTools, no_aliases;
    "Bertbox": "Set up and inspect bertbox itself" {
        "completions" => bertbox::Completions,
        "install" => bertbox::Install,
        "list" => bertbox::List,
    }
}

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

impl Cli {
    fn command_with_flags() -> Command {
        Cli::command().mut_subcommands(env_file::with_flag)
    }
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
    let build = || {
        if as_bertbox {
            env_file::with_flag(Bertbox::command())
        } else {
            Cli::command_with_flags()
        }
    };
    if let Err(error) = env_file::load(&build()) {
        eprintln!("Error: {error:?}");
        return ExitCode::FAILURE;
    }
    // clap snapshots `Arg::env` values when the command is built.
    let command = build();
    if as_bertbox {
        return parse::<Bertbox>(command).tool.run();
    }
    match parse::<Cli>(command) {
        Cli::Bertbox(Bertbox { tool }) => tool.run(),
        Cli::Tool(tool) => tool.run(),
    }
}

fn parse<T: FromArgMatches>(mut command: Command) -> T {
    let mut matches = command.get_matches_mut();
    T::from_arg_matches_mut(&mut matches).unwrap_or_else(|error| error.format(&mut command).exit())
}
