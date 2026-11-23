use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::install_target::InstallTarget;
use crate::tools::bertbox::packages::{Removal, Version, files, paths};

const BIN: &str = "bin";
const MANIFEST: &str = "version.json";

pub struct Package {
    name: String,
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    target: InstallTarget,
    version: Version,
    files: BTreeMap<PathBuf, String>,
}

pub fn installed() -> Result<Vec<Package>> {
    let pkgs = paths::pkgs_dir()?;
    let entries = match fs::read_dir(&pkgs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", pkgs.display())),
    };
    let mut packages = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", pkgs.display()))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        packages.push(Package {
            name,
            dir: entry.path(),
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

impl Package {
    pub fn new(name: &str) -> Result<Package> {
        Ok(Package {
            name: name.to_owned(),
            dir: paths::pkgs_dir()?.join(name),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn install(
        &self,
        target: InstallTarget,
        version: Version,
        executables: Vec<(PathBuf, Vec<u8>)>,
    ) -> Result<()> {
        let files = executables
            .iter()
            .map(|(name, data)| (name.clone(), hex::encode(Sha256::digest(data))))
            .collect();
        self.replace_executable(executables)?;
        let manifest = Manifest {
            target,
            version,
            files,
        };
        let rendered = serde_json::to_string_pretty(&manifest).context("rendering the manifest")?;
        files::write(&self.manifest_path(), format!("{rendered}\n").as_bytes())
    }

    pub fn replace_executable(&self, executables: Vec<(PathBuf, Vec<u8>)>) -> Result<()> {
        let bin = self.bin();
        fs::create_dir_all(&bin).with_context(|| format!("creating {}", bin.display()))?;
        let mut keep = Vec::new();
        for (name, data) in &executables {
            if name.file_name() != Some(name.as_os_str()) {
                bail!("{}: not a file name", name.display());
            }
            let dest = bin.join(name);
            files::write_executable(&dest, data)?;
            println!("Installed: {}", dest.display());
            keep.push(name.as_os_str());
        }
        for entry in fs::read_dir(&bin).with_context(|| format!("reading {}", bin.display()))? {
            let entry = entry.with_context(|| format!("reading {}", bin.display()))?;
            if !keep.contains(&entry.file_name().as_os_str()) {
                fs::remove_file(entry.path())
                    .with_context(|| format!("removing {}", entry.path().display()))?;
            }
        }
        self.link()
    }

    pub fn is_version(&self, target: InstallTarget, version: &Version) -> Result<bool> {
        let Some(manifest) = self.manifest()? else {
            return Ok(false);
        };
        if manifest.target != target || manifest.version != *version {
            return Ok(false);
        }
        if let Err(error) = self.verify_contents(&manifest) {
            println!("{error:#}");
            return Ok(false);
        }
        Ok(true)
    }

    pub fn link(&self) -> Result<()> {
        let bin_dir = paths::bin_dir()?;
        fs::create_dir_all(&bin_dir).with_context(|| format!("creating {}", bin_dir.display()))?;
        let bin = self.bin();
        let entries = fs::read_dir(&bin).with_context(|| format!("reading {}", bin.display()))?;
        for entry in entries {
            let target = entry
                .with_context(|| format!("reading {}", bin.display()))?
                .path();
            let Some(file_name) = target.file_name() else {
                continue;
            };
            let link = bin_dir.join(file_name);
            if fs::read_link(&link).is_ok_and(|current| current == target) {
                continue;
            }
            if link.symlink_metadata().is_ok() {
                fs::remove_file(&link).with_context(|| format!("removing {}", link.display()))?;
            }
            symlink(&target, &link)
                .with_context(|| format!("linking {} to {}", link.display(), target.display()))?;
        }
        self.unlink(|target| !target.exists())
    }

    pub fn remove(&self) -> Result<Removal> {
        if !self.dir.exists() {
            return Ok(Removal::NotInstalled);
        }
        self.unlink(|_| true)?;
        fs::remove_dir_all(&self.dir)
            .with_context(|| format!("removing {}", self.dir.display()))?;
        println!("Removed: {}", self.dir.display());
        Ok(Removal::Removed)
    }

    pub fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        let path = self.bin().join(program);
        let status = Command::new(&path)
            .args(args)
            .status()
            .with_context(|| format!("running {}", path.display()))?;
        if !status.success() {
            bail!("{} {} failed: {status}", path.display(), args.join(" "));
        }
        Ok(())
    }

    fn verify_contents(&self, manifest: &Manifest) -> Result<()> {
        let bin = self.bin();
        let mut on_disk = BTreeMap::new();
        for entry in fs::read_dir(&bin).with_context(|| format!("reading {}", bin.display()))? {
            let entry = entry.with_context(|| format!("reading {}", bin.display()))?;
            let data = fs::read(entry.path())
                .with_context(|| format!("reading {}", entry.path().display()))?;
            on_disk.insert(
                PathBuf::from(entry.file_name()),
                hex::encode(Sha256::digest(data)),
            );
        }
        if on_disk != manifest.files {
            bail!("{}: installed files do not match the manifest", self.name);
        }
        Ok(())
    }

    fn manifest(&self) -> Result<Option<Manifest>> {
        let path = self.manifest_path();
        match fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).ok()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.dir.join(MANIFEST)
    }

    fn bin(&self) -> PathBuf {
        self.dir.join(BIN)
    }

    fn unlink(&self, remove: impl Fn(&Path) -> bool) -> Result<()> {
        let bin_dir = paths::bin_dir()?;
        let entries = match fs::read_dir(&bin_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", bin_dir.display()));
            }
        };
        for entry in entries {
            let path = entry
                .with_context(|| format!("reading {}", bin_dir.display()))?
                .path();
            let Ok(target) = fs::read_link(&path) else {
                continue;
            };
            let target = bin_dir.join(target);
            if target.starts_with(&self.dir) && remove(&target) {
                fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
        }
        Ok(())
    }
}
