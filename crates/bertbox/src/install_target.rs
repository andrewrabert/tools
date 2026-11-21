use std::ffi::CStr;
use std::fmt;
use std::mem::MaybeUninit;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::which;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    Linux,
    Macos,
    Termux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Flatpak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallTarget(pub System, pub Arch, pub Option<PackageManager>);

impl InstallTarget {
    pub fn host() -> Result<Self> {
        let (sysname, machine) = uname()?;
        let arch = match machine.as_str() {
            "x86_64" => Arch::X86_64,
            "aarch64" | "arm64" => Arch::Aarch64,
            _ => bail!("unsupported platform: {sysname} {machine}"),
        };
        let system = match sysname.as_str() {
            "Darwin" => System::Macos,
            "Linux" if which::first("termux-info").is_some() => System::Termux,
            "Linux" => System::Linux,
            _ => bail!("unsupported platform: {sysname} {machine}"),
        };
        Ok(InstallTarget(system, arch, None))
    }
}

impl fmt::Display for InstallTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let InstallTarget(system, arch, package_manager) = self;
        write!(f, "{system:?} {arch:?}")?;
        match package_manager {
            Some(package_manager) => write!(f, " {package_manager:?}"),
            None => Ok(()),
        }
    }
}

fn uname() -> Result<(String, String)> {
    let mut name = MaybeUninit::<libc::utsname>::uninit();
    if unsafe { libc::uname(name.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let name = unsafe { name.assume_init() };
    let field = |chars: &[libc::c_char]| {
        unsafe { CStr::from_ptr(chars.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    Ok((field(&name.sysname[..]), field(&name.machine[..])))
}
