use std::path::Path;

const TEXT: &str = "text/plain";
const TEXT_SUFFIX: &str = ".txt";
const UNKNOWN_SUFFIX: &str = ".unknown";

/// The suffixes that fit each MIME type, the one `--fix` appends first.
const SUFFIXES: &[(&str, &[&str])] = &[
    ("application/gzip", &[".gz"]),
    ("application/pdf", &[".pdf"]),
    ("application/vnd.microsoft.portable-executable", &[".exe"]),
    ("application/x-7z-compressed", &[".7z"]),
    ("application/x-ace-compressed", &[".ace"]),
    ("application/x-bittorrent", &[".torrent"]),
    ("application/x-dosexec", &[".exe"]),
    ("application/x-rar", &[".rar"]),
    ("application/x-tar", &[".tar"]),
    ("application/zip", &[".zip"]),
    ("audio/amr", &[".amr"]),
    ("audio/flac", &[".flac"]),
    ("audio/mpeg", &[".mp3"]),
    ("audio/x-m4a", &[".m4a"]),
    ("image/avif", &[".avif"]),
    ("image/bmp", &[".bmp"]),
    ("image/gif", &[".gif"]),
    ("image/heic", &[".heic"]),
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

pub enum Kind {
    Known(&'static [&'static str]),
    Text,
    Unknown,
}

pub enum Verdict {
    Fits,
    /// The suffix to append.
    Wrong(&'static str),
    /// Plain text under a suffix that belongs to another type; only `--force-fix` renames it.
    Unfixable,
}

impl Kind {
    pub fn of(mime: &str) -> Kind {
        if mime == TEXT {
            return Kind::Text;
        }
        SUFFIXES
            .iter()
            .find(|(known, _)| *known == mime)
            .map_or(Kind::Unknown, |(_, suffixes)| Kind::Known(suffixes))
    }

    /// Whether a lowercased suffix fits this kind.
    pub fn judge(&self, suffix: &str, force: bool) -> Verdict {
        match self {
            Kind::Known(suffixes) if suffixes.contains(&suffix) => Verdict::Fits,
            Kind::Known(suffixes) => Verdict::Wrong(suffixes[0]),
            Kind::Text if force => expect(suffix, TEXT_SUFFIX),
            Kind::Text if belongs_to_any(suffix) => Verdict::Unfixable,
            Kind::Text => Verdict::Fits,
            Kind::Unknown if force => expect(suffix, UNKNOWN_SUFFIX),
            Kind::Unknown => Verdict::Fits,
        }
    }
}

fn expect(suffix: &str, wanted: &'static str) -> Verdict {
    if suffix == wanted {
        Verdict::Fits
    } else {
        Verdict::Wrong(wanted)
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
