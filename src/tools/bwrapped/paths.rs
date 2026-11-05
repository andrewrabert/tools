//! Paths made absolute with every existing symlink followed.
use std::fs;
use std::path::{self, Component, Path, PathBuf};

/// `path` made absolute against the working directory, with symlinks followed as far as the
/// path exists. The rest is kept as written, so a path that does not exist yet still resolves.
pub fn resolve(path: &Path) -> PathBuf {
    let Ok(absolute) = path::absolute(path) else {
        return path.to_owned();
    };
    let mut resolved = PathBuf::from("/");
    let mut exists = true;
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                if exists {
                    match fs::canonicalize(&resolved) {
                        Ok(real) => resolved = real,
                        Err(_) => exists = false,
                    }
                }
            }
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn symlinks_are_followed_and_missing_parts_kept() {
        let dir = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir(real.join("target")).unwrap();
        symlink(real.join("target"), real.join("link")).unwrap();

        assert_eq!(
            resolve(&real.join("link/missing/../file")),
            real.join("target/file")
        );
        assert_eq!(resolve(&real.join("link/./a")), real.join("target/a"));
    }
}
