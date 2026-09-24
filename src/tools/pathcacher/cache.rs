use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result};
use md5::{Digest, Md5};
use tempfile::NamedTempFile;

pub struct Key(String);

pub struct Cache {
    root: PathBuf,
}

impl Key {
    pub fn of(path: &Path, name: &str) -> Result<Self> {
        let path = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
        let metadata = fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let path_hash = Md5::digest(path.as_os_str().as_bytes());
        Ok(Key(format!(
            "{path_hash:x}_{}_{}_{name}",
            metadata.size(),
            metadata.mtime()
        )))
    }
}

impl Cache {
    pub fn open(cache_home: &Path) -> Result<Self> {
        let root = cache_home.join("pathcacher");
        fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
        Ok(Cache { root })
    }

    pub fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        let path = self.root.join(&key.0);
        match fs::read(&path) {
            Ok(data) => Ok(Some(data)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn set(&self, key: &Key, data: &[u8]) -> Result<()> {
        let path = self.root.join(&key.0);
        let mut file = NamedTempFile::new_in(&self.root)
            .with_context(|| format!("creating a temporary file in {}", self.root.display()))?;
        file.write_all(data)
            .with_context(|| format!("writing {}", file.path().display()))?;
        file.persist(&path)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
