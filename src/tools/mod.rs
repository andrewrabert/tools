pub mod archive;
pub mod b64;
pub mod bertbox;
pub mod cb;
pub mod code_nuke;
pub mod dl;
pub mod dotfiles_link_bin;
pub mod empty_tree;
pub mod encode_utf8;
pub mod escape_files_windows;
pub mod fanpipe;
pub mod file_hash_recorder;
pub mod find_dupes;
pub mod find_file_exts;
pub mod full_update;
pub mod geoip;
pub mod img;
pub mod json;
pub mod lmk;
pub mod mac_address;
pub mod mkv_preview;
pub mod mkv_set_title_from_filename;
pub mod music_organizer;
pub mod path_renamer;
pub mod pathbin;
pub mod pathcacher;
pub mod preview;
pub mod publicip;
pub mod radix;
pub mod reencode_lossless;
pub mod rerename;
pub mod txt;
pub mod url;
pub mod urlextract;
pub mod zlib;

use std::process::ExitCode;

use clap::Args;

pub trait Tool: Args {
    fn run(self) -> ExitCode;
}
