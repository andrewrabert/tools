use std::ffi::OsString;
use std::path::Path;

use clap::ValueEnum;

#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    #[value(name = "7z")]
    SevenZip,
    Cbz,
    Epub,
    Iso,
    Gz,
    Nsz,
    Rvz,
    Tar,
    #[value(name = "tar.gz")]
    TarGz,
    #[value(name = "tar.xz")]
    TarXz,
    #[value(name = "tar.zst")]
    TarZst,
    Xz,
    Zip,
    Zst,
    #[value(name = "sparse:tar.zst")]
    SparseTarZst,
}

impl Format {
    pub fn suffix(self) -> &'static str {
        match self {
            Format::SevenZip => ".7z",
            Format::Cbz => ".cbz",
            Format::Epub => ".epub",
            Format::Iso => ".iso",
            Format::Gz => ".gz",
            Format::Nsz => ".nsz",
            Format::Rvz => ".rvz",
            Format::Tar => ".tar",
            Format::TarGz => ".tar.gz",
            Format::TarXz => ".tar.xz",
            Format::TarZst | Format::SparseTarZst => ".tar.zst",
            Format::Xz => ".xz",
            Format::Zip => ".zip",
            Format::Zst => ".zst",
        }
    }

    pub fn base_name(self, source: &Path) -> OsString {
        let name = source.file_name().unwrap_or(source.as_os_str());
        match self {
            Format::Nsz => {
                let is_nsp = source
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("nsp"));
                match (is_nsp, source.file_stem()) {
                    (true, Some(stem)) => stem.to_owned(),
                    _ => name.to_owned(),
                }
            }
            _ => name.to_owned(),
        }
    }

    pub fn protects_with_password(self) -> bool {
        matches!(self, Format::SevenZip)
    }
}
