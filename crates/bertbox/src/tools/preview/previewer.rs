use std::path::Path;

use crate::tools::preview::mime::MimeType;

pub enum Previewer {
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

impl Previewer {
    pub fn for_file(mime: &MimeType, file: &Path) -> Self {
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
            | "application/zstd" => Previewer::Archive,
            "application/x-cue" => Previewer::CueSheet,
            "application/java-archive" => Previewer::JavaArchive,
            // xdg-mime reports go.mod files as audio/x-mod
            "audio/x-mod" | "audio/x-mpegurl" => Previewer::Text,
            "video/matroska" | "video/x-matroska" => Previewer::Matroska,
            "application/epub+zip" => Previewer::Epub,
            "application/vnd.microsoft.portable-executable"
            | "application/x-dosexec"
            | "application/x-ms-dos-executable"
            | "application/x-msdownload" => Previewer::Executable,
            "application/pdf" | "image/gif" | "image/x-canon-cr2" | "image/x-canon-cr3" => {
                Previewer::Exif
            }
            "application/vnd.iccprofile" => Previewer::IccProfile,
            "image/jxl" => Previewer::Jxl,
            "image/tiff" => Previewer::Tiff,
            "image/x-portable-pixmap" | "image/vnd.microsoft.icon" => Previewer::BareImage,
            "application/octet-stream" | "" => Previewer::for_unrecognized(file),
            other if other.starts_with("audio/") || other.starts_with("video/") => Previewer::Media,
            other if other.starts_with("image/") => Previewer::Image,
            _ => Previewer::Text,
        }
    }

    fn for_unrecognized(file: &Path) -> Self {
        let extension = file
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        match extension.as_deref() {
            Some("rvz") => Previewer::Archive,
            Some("umx") => Previewer::Media,
            _ => Previewer::Binary,
        }
    }
}
