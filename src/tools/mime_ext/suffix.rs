use std::path::Path;

use crate::mime::MimeType;

const TEXT_SUFFIX: &str = ".txt";
const UNKNOWN_SUFFIX: &str = ".unknown";

/// The suffixes that fit each MIME type as `infer` names it, the one `--fix` appends first.
const SUFFIXES: &[(&str, &[&str])] = &[
    ("application/gzip", &[".gz"]),
    ("application/pdf", &[".pdf"]),
    ("application/vnd.microsoft.portable-executable", &[".exe"]),
    ("application/vnd.rar", &[".rar"]),
    ("application/x-7z-compressed", &[".7z"]),
    ("application/x-ace-compressed", &[".ace"]),
    ("application/x-bittorrent", &[".torrent"]),
    ("application/x-tar", &[".tar"]),
    ("application/zip", &[".zip"]),
    ("audio/amr", &[".amr"]),
    ("audio/m4a", &[".m4a"]),
    ("audio/mpeg", &[".mp3"]),
    ("audio/x-flac", &[".flac"]),
    ("image/avif", &[".avif"]),
    ("image/bmp", &[".bmp"]),
    ("image/gif", &[".gif"]),
    ("image/heif", &[".heic"]),
    ("image/jpeg", &[".jpg", ".jpeg"]),
    ("image/png", &[".png"]),
    ("image/tiff", &[".tiff", ".tif"]),
    ("image/webp", &[".webp"]),
    ("image/x-canon-cr2", &[".cr2"]),
    ("image/x-canon-cr3", &[".cr3"]),
    ("text/html", &[".html"]),
    ("video/mp2t", &[".mp2t"]),
    ("video/mp4", &[".mp4"]),
    ("video/quicktime", &[".mov"]),
    ("video/x-m4v", &[".m4v"]),
];

pub enum Expected {
    Suffixes(&'static [&'static str]),
    Text,
    Unrecognized,
}

pub enum Fix {
    Keep,
    Append(&'static str),
    /// Plain text under a suffix that belongs to another type; only `--force-fix` renames it.
    Unfixable,
}

impl Expected {
    pub fn of(mime: &MimeType) -> Expected {
        match mime {
            MimeType::Text => Expected::Text,
            MimeType::Binary => Expected::Unrecognized,
            MimeType::Other(name) => SUFFIXES
                .iter()
                .find(|(known, _)| *known == name.as_str())
                .map_or(Expected::Unrecognized, |(_, suffixes)| {
                    Expected::Suffixes(suffixes)
                }),
        }
    }

    /// `suffix` is lowercased.
    pub fn fix(&self, suffix: &str, force: bool) -> Fix {
        match self {
            Expected::Suffixes(suffixes) if suffixes.contains(&suffix) => Fix::Keep,
            Expected::Suffixes(suffixes) => Fix::Append(suffixes[0]),
            Expected::Text if force => append_unless(suffix, TEXT_SUFFIX),
            Expected::Text if belongs_to_any(suffix) => Fix::Unfixable,
            Expected::Text => Fix::Keep,
            Expected::Unrecognized if force => append_unless(suffix, UNKNOWN_SUFFIX),
            Expected::Unrecognized => Fix::Keep,
        }
    }
}

fn append_unless(suffix: &str, wanted: &'static str) -> Fix {
    if suffix == wanted {
        Fix::Keep
    } else {
        Fix::Append(wanted)
    }
}

fn belongs_to_any(suffix: &str) -> bool {
    SUFFIXES
        .iter()
        .any(|(_, suffixes)| suffixes.contains(&suffix))
}

/// The lowercased final suffix with its dot, or empty when there is none.
pub fn of(path: &Path) -> String {
    match path.extension() {
        Some(extension) if !extension.is_empty() => {
            format!(".{}", extension.to_string_lossy().to_lowercase())
        }
        _ => String::new(),
    }
}
