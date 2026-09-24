pub mod cabextract;
pub mod cpio;
pub mod cue_sheet;
pub mod dolphin;
pub mod gnu_tar;
pub mod gog_linux;
pub mod innoextract;
pub mod nsz;
pub mod seven_zip;
pub mod stream;
pub mod udisksctl;
pub mod unace;
pub mod unarchiver;
pub mod unrar;
pub mod unshield;
pub mod unsquashfs;
pub mod unzip;
pub mod xorriso;

use std::ffi::OsStr;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::tools::archive::extract::config::Overwrite;
use crate::tools::archive::extract::listing::Contents;
use crate::tools::archive::password::Password;
use crate::tools::archive::process;

pub const TEMP_PREFIX: &str = ".extract_";

pub enum Destination<'a> {
    Directory(&'a Path),
    Stream(&'a OwnedFd),
}

pub struct Request<'a> {
    pub archive: &'a Path,
    pub destination: Destination<'a>,
    pub members: &'a [String],
    pub password: Option<&'a Password>,
    pub overwrite: Overwrite,
}

impl Request<'_> {
    pub fn directory(&self) -> Result<&Path> {
        match self.destination {
            Destination::Directory(directory) => Ok(directory),
            Destination::Stream(_) => bail!("this archive type cannot be extracted to stdout"),
        }
    }

    pub fn every_member(&self) -> Result<()> {
        if !self.members.is_empty() {
            bail!("this archive type cannot extract single members");
        }
        Ok(())
    }

    pub fn keeping_existing(&self) -> Result<()> {
        if let Overwrite::Replace = self.overwrite {
            bail!("this archive type cannot overwrite existing files");
        }
        Ok(())
    }

    pub fn without_password(&self) -> Result<()> {
        if self.password.is_some() {
            bail!("this archive type cannot take a password");
        }
        Ok(())
    }
}

pub fn file_name(archive: &Path) -> Result<&OsStr> {
    archive
        .file_name()
        .with_context(|| format!("{} has no file name", archive.display()))
}

pub fn file_stem(archive: &Path) -> Result<&OsStr> {
    archive
        .file_stem()
        .with_context(|| format!("{} has no file name", archive.display()))
}

#[derive(Clone, Copy, Debug)]
pub enum Backend {
    Brotli,
    Bzip2,
    Cabextract,
    Cpio,
    CueSheet,
    DolphinGcm,
    DolphinRvz,
    GnuTar,
    GogLinux,
    Gzip,
    Innoextract,
    Lz4,
    Nsz,
    Pbzip2,
    Pigz,
    Pixz,
    SevenZip,
    Udisksctl,
    Unace,
    Unarchiver,
    Unrar,
    Unshield,
    Unsquashfs,
    Unzip,
    Xorriso,
    Xz,
    Zstd,
}

impl Backend {
    pub fn check(self) -> Result<()> {
        let programs: &[&str] = match self {
            Backend::SevenZip => return seven_zip::program().map(drop),
            Backend::GnuTar => return gnu_tar::check(),
            Backend::GogLinux => &[],
            Backend::Brotli => &["brotli"],
            Backend::Bzip2 => &["bzip2"],
            Backend::Cabextract => &["cabextract"],
            Backend::Cpio => &["cpio"],
            Backend::CueSheet => &["bchunk", "binmerge"],
            Backend::DolphinGcm | Backend::DolphinRvz => &["dolphin-tool"],
            Backend::Gzip => &["gzip"],
            // unrar unpacks the .bin volumes of GOG installers
            Backend::Innoextract => &["innoextract", "unrar"],
            Backend::Lz4 => &["lz4"],
            Backend::Nsz => &["nsz"],
            Backend::Pbzip2 => &["pbzip2"],
            Backend::Pigz => &["pigz"],
            Backend::Pixz => &["pixz"],
            Backend::Udisksctl => &["udisksctl"],
            Backend::Unace => &["unace"],
            Backend::Unarchiver => &["lsar", "unar"],
            Backend::Unrar => &["unrar"],
            Backend::Unshield => &["unshield"],
            Backend::Unsquashfs => &["unsquashfs"],
            Backend::Unzip => &["unzip"],
            Backend::Xorriso => &["xorriso"],
            Backend::Xz => &["xz"],
            Backend::Zstd => &["zstd"],
        };
        programs
            .iter()
            .try_for_each(|program| process::require(program))
    }

    pub fn contents(self, archive: &Path) -> Result<Contents> {
        match self {
            Backend::Brotli
            | Backend::Bzip2
            | Backend::Gzip
            | Backend::Lz4
            | Backend::Pbzip2
            | Backend::Pigz
            | Backend::Pixz
            | Backend::Xz
            | Backend::Zstd => stream::contents(archive),
            Backend::Cabextract => cabextract::contents(archive),
            Backend::Cpio => cpio::contents(archive),
            Backend::CueSheet => cue_sheet::contents(archive),
            Backend::DolphinGcm => dolphin::gcm_contents(archive),
            Backend::DolphinRvz => dolphin::rvz_contents(archive),
            Backend::GnuTar => gnu_tar::contents(archive),
            Backend::GogLinux => gog_linux::contents(archive),
            Backend::Innoextract => innoextract::contents(archive),
            Backend::Nsz => nsz::contents(archive),
            Backend::SevenZip => seven_zip::contents(archive),
            Backend::Udisksctl => udisksctl::contents(archive),
            Backend::Unace => unace::contents(archive),
            Backend::Unarchiver => unarchiver::contents(archive),
            Backend::Unrar => unrar::contents(archive),
            Backend::Unshield => unshield::contents(archive),
            Backend::Unsquashfs => unsquashfs::contents(archive),
            Backend::Unzip => unzip::contents(archive),
            Backend::Xorriso => xorriso::contents(archive),
        }
    }

    pub fn volumes(self, archive: &Path, password: Option<&Password>) -> Result<Vec<PathBuf>> {
        match self {
            Backend::CueSheet => {
                if password.is_some() {
                    bail!("cue sheets cannot take a password");
                }
                cue_sheet::volumes(archive)
            }
            Backend::Innoextract => Ok(innoextract::volumes(archive)),
            Backend::SevenZip => seven_zip::volumes(archive, password),
            Backend::Unarchiver => unarchiver::volumes(archive),
            Backend::Unrar => unrar::volumes(archive),
            _ => Ok(vec![archive.to_owned()]),
        }
    }

    pub fn extract(self, request: &Request) -> Result<()> {
        match self {
            Backend::Brotli => stream::brotli(request),
            Backend::Bzip2 => stream::bzip2(request),
            Backend::Cabextract => cabextract::extract(request),
            Backend::Cpio => cpio::extract(request),
            Backend::CueSheet => cue_sheet::extract(request),
            Backend::DolphinGcm => dolphin::gcm_extract(request),
            Backend::DolphinRvz => dolphin::rvz_extract(request),
            Backend::GnuTar => gnu_tar::extract(request),
            Backend::GogLinux => gog_linux::extract(request),
            Backend::Gzip => stream::gzip(request),
            Backend::Innoextract => innoextract::extract(request),
            Backend::Lz4 => stream::lz4(request),
            Backend::Nsz => nsz::extract(request),
            Backend::Pbzip2 => stream::pbzip2(request),
            Backend::Pigz => stream::pigz(request),
            Backend::Pixz => stream::pixz(request),
            Backend::SevenZip => seven_zip::extract(request),
            Backend::Udisksctl => udisksctl::extract(request),
            Backend::Unace => unace::extract(request),
            Backend::Unarchiver => unarchiver::extract(request),
            Backend::Unrar => unrar::extract(request),
            Backend::Unshield => unshield::extract(request),
            Backend::Unsquashfs => unsquashfs::extract(request),
            Backend::Unzip => unzip::extract(request),
            Backend::Xorriso => xorriso::extract(request),
            Backend::Xz => stream::xz(request),
            Backend::Zstd => stream::zstd(request),
        }
    }
}
