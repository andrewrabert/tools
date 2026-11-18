//! Source strings that name local paths, compared by where they resolve.
use std::fs;
use std::path::{self, Component, Path, PathBuf};

use anyhow::{Context, Result};
use nix::unistd::User;

use crate::dirs;

pub fn expand_user(path: &str) -> Result<PathBuf> {
    let Some(rest) = path.strip_prefix('~') else {
        return Ok(PathBuf::from(path));
    };
    let (user, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let home = if user.is_empty() {
        dirs::home()?
    } else {
        let entry = User::from_name(user).with_context(|| format!("looking up user {user}"))?;
        match entry {
            Some(entry) => entry.dir,
            None => return Ok(PathBuf::from(path)),
        }
    };
    Ok(home.join(tail.trim_start_matches('/')))
}

/// `path` made absolute with every symlink resolved; components that do not exist are kept as written.
pub fn resolve(path: &Path) -> Result<PathBuf> {
    let absolute = path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            component => {
                resolved.push(component);
                if resolved.is_symlink()
                    && let Ok(real) = fs::canonicalize(&resolved)
                {
                    resolved = real;
                }
            }
        }
    }
    Ok(resolved)
}

pub fn local_path(source: &str) -> Result<Option<PathBuf>> {
    let local = matches!(source, "." | "..")
        || ["/", "./", "../", "~"]
            .iter()
            .any(|prefix| source.starts_with(prefix));
    if local {
        expand_user(source).map(Some)
    } else {
        Ok(None)
    }
}

pub fn normalize_source(source: &str) -> Result<String> {
    match local_path(source)? {
        Some(local) => Ok(resolve(&local)?.display().to_string()),
        None => Ok(source.to_owned()),
    }
}

pub fn collapse_home(path: &str) -> Result<String> {
    let home = dirs::home()?;
    Ok(match Path::new(path).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn remote_sources_are_not_local() {
        assert_eq!(local_path("owner/repo").unwrap(), None);
        assert_eq!(local_path("https://example.com/x.git").unwrap(), None);
        assert_eq!(normalize_source("owner/repo").unwrap(), "owner/repo");
    }

    #[test]
    fn local_sources_are_recognized() {
        for source in [".", "..", "/a", "./a", "../a", "~", "~/a"] {
            assert!(local_path(source).unwrap().is_some(), "{source}");
        }
    }

    #[test]
    fn tilde_expands_to_home() {
        let home = dirs::home().unwrap();
        assert_eq!(expand_user("~").unwrap(), home);
        assert_eq!(expand_user("~/a/b").unwrap(), home.join("a/b"));
        assert_eq!(expand_user("a/~").unwrap(), PathBuf::from("a/~"));
    }

    #[test]
    fn collapse_home_reverses_expansion() {
        let home = dirs::home().unwrap();
        let inside = home.join("a/b").display().to_string();
        assert_eq!(collapse_home(&inside).unwrap(), "~/a/b");
        assert_eq!(collapse_home(&home.display().to_string()).unwrap(), "~");
        assert_eq!(collapse_home("/nonexistent/x").unwrap(), "/nonexistent/x");
    }

    #[test]
    fn resolve_follows_symlinks_and_keeps_missing_components() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        fs::create_dir(root.join("real")).unwrap();
        symlink(root.join("real"), root.join("link")).unwrap();
        assert_eq!(
            resolve(&root.join("link/missing/../other")).unwrap(),
            root.join("real/other")
        );
    }
}
