use std::path::Path;

use crate::tools::preview::mime::MimeType;

pub enum Kind {
    Archive,
    CueSheet,
    JavaArchive,
    Text,
    Matroska,
    Media,
    Epub,
    Executable,
    Exif,
    IccProfile,
    Jxl,
    Tiff,
    BareImage,
    Image,
    Binary,
}

impl Kind {
    pub fn of(mime: &MimeType, file: &Path) -> Self {
        match mime.as_str() {
            "application/gzip"
            | "application/x-rpm"
            | "application/x-stuffit"
            | "application/vnd.comicbook+zip"
            | "application/vnd.comicbook-rar"
            | "application/vnd.efi.img"
            | "application/vnd.efi.iso"
            | "application/vnd.google-earth.kmz"
            | "application/vnd.ms-cab-compressed"
            | "application/vnd.rar"
            | "application/x-7z-compressed"
            | "application/x-ace"
            | "application/x-bzip-compressed-tar"
            | "application/x-bzip2-compressed-tar"
            | "application/x-cd-image"
            | "application/x-compressed-tar"
            | "application/x-iso9660-image"
            | "application/x-pak"
            | "application/x-rar"
            | "application/x-raw-disk-image"
            | "application/x-tar"
            | "application/x-xz-compressed-tar"
            | "application/x-zstd-compressed-tar"
            | "application/zip"
            | "application/zlib"
            | "application/zstd" => Kind::Archive,
            "application/x-cue" => Kind::CueSheet,
            "application/java-archive" => Kind::JavaArchive,
            // xdg-mime reports go.mod files as audio/x-mod
            "audio/x-mod" | "audio/x-mpegurl" => Kind::Text,
            "video/matroska" | "video/x-matroska" => Kind::Matroska,
            "application/epub+zip" => Kind::Epub,
            "application/vnd.microsoft.portable-executable"
            | "application/x-dosexec"
            | "application/x-ms-dos-executable"
            | "application/x-msdownload" => Kind::Executable,
            "application/pdf" | "image/gif" | "image/x-canon-cr2" | "image/x-canon-cr3" => {
                Kind::Exif
            }
            "application/vnd.iccprofile" => Kind::IccProfile,
            "image/jxl" => Kind::Jxl,
            "image/tiff" => Kind::Tiff,
            "image/x-portable-pixmap" | "image/vnd.microsoft.icon" => Kind::BareImage,
            "application/octet-stream" | "" => Kind::of_unrecognized(file),
            other if other.starts_with("audio/") || other.starts_with("video/") => Kind::Media,
            other if other.starts_with("image/") => Kind::Image,
            _ => Kind::Text,
        }
    }

    fn of_unrecognized(file: &Path) -> Self {
        let extension = file
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        match extension.as_deref() {
            Some("rvz") => Kind::Archive,
            Some("umx") => Kind::Media,
            _ => Kind::Binary,
        }
    }
}
