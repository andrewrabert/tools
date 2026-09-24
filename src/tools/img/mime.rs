use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const HEADER_LEN: usize = 12;
const JXL_CONTAINER: &[u8] = b"\x00\x00\x00\x0cJXL \x0d\x0a\x87\x0a";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mime {
    Krita,
    Bmp,
    Gif,
    Heif,
    Jpeg,
    Jxl,
    Png,
    Svg,
    Tiff,
    Webp,
    Ppm,
}

impl Mime {
    /// The mime type string, which the optimization cache also stores.
    pub fn as_str(self) -> &'static str {
        match self {
            Mime::Krita => "application/x-krita",
            Mime::Bmp => "image/bmp",
            Mime::Gif => "image/gif",
            Mime::Heif => "image/heif",
            Mime::Jpeg => "image/jpeg",
            Mime::Jxl => "image/jxl",
            Mime::Png => "image/png",
            Mime::Svg => "image/svg+xml",
            Mime::Tiff => "image/tiff",
            Mime::Webp => "image/webp",
            Mime::Ppm => "image/x-portable-pixmap",
        }
    }

    /// File name suffixes, preferred first.
    pub fn suffixes(self) -> &'static [&'static str] {
        match self {
            Mime::Krita => &[".kra"],
            Mime::Bmp => &[".bmp"],
            Mime::Gif => &[".gif"],
            Mime::Heif => &[".heic"],
            Mime::Jpeg => &[".jpg", ".jpeg"],
            Mime::Jxl => &[".jxl"],
            Mime::Png => &[".png"],
            Mime::Svg => &[".svg"],
            Mime::Tiff => &[".tiff", ".tif"],
            Mime::Webp => &[".webp"],
            Mime::Ppm => &[".ppm"],
        }
    }

    pub fn suffix(self) -> &'static str {
        self.suffixes()[0]
    }

    /// Whether one file of this type can hold several pages (`convert --combine`).
    pub fn is_multipage(self) -> bool {
        matches!(self, Mime::Tiff)
    }

    /// Whether metadata is copied from a source of this type into a JPEG target.
    pub fn carries_jpeg_metadata(self) -> bool {
        matches!(
            self,
            Mime::Heif | Mime::Jpeg | Mime::Jxl | Mime::Png | Mime::Tiff | Mime::Webp
        )
    }
}

/// The types `optim` handles when none are given.
pub const DEFAULT_OPTIM: [Mime; 3] = [Mime::Gif, Mime::Jpeg, Mime::Png];

#[derive(Clone, Copy, Debug)]
pub struct Detected {
    pub mime: Mime,
    pub bigtiff: bool,
}

/// Sniffs the type from the first bytes, falling back to the suffix for text formats.
pub fn detect(path: &Path) -> io::Result<Option<Detected>> {
    let mut header = [0u8; HEADER_LEN];
    let mut file = File::open(path)?;
    let mut filled = 0;
    while filled < HEADER_LEN {
        let read = file.read(&mut header[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    let header = &header[..filled];
    let at = |start: usize, end: usize| header.get(start..end).unwrap_or_default();

    let plain = |mime| {
        Some(Detected {
            mime,
            bigtiff: false,
        })
    };
    let suffix = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    Ok(if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        plain(Mime::Png)
    } else if header.starts_with(b"P6\n") {
        plain(Mime::Ppm)
    } else if header.starts_with(b"\xff\xd8\xff") {
        plain(Mime::Jpeg)
    } else if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        plain(Mime::Gif)
    } else if header.starts_with(b"\x49\x49\x2b\x00") {
        Some(Detected {
            mime: Mime::Tiff,
            bigtiff: true,
        })
    } else if header.starts_with(b"\x49\x49\x2a\x00") || header.starts_with(b"\x4d\x4d\x00\x2a") {
        plain(Mime::Tiff)
    } else if header.starts_with(b"<svg ") || header.starts_with(b"<?svg ") {
        plain(Mime::Svg)
    } else if header.starts_with(JXL_CONTAINER) || header.starts_with(b"\xff\x0a") {
        plain(Mime::Jxl)
    } else if at(4, 12) == b"ftypheic" {
        plain(Mime::Heif)
    } else if header.starts_with(b"RIFF") && at(8, 12) == b"WEBP" {
        plain(Mime::Webp)
    } else if header.starts_with(b"BM") {
        plain(Mime::Bmp)
    } else if suffix.as_deref() == Some("svg") {
        plain(Mime::Svg)
    } else if suffix.as_deref() == Some("kra") {
        plain(Mime::Krita)
    } else {
        None
    })
}
