use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::tools::base16_shell_preview::repo::Theme;

const THEME_LINK: &str = ".base16_theme";
const FISH_THEME_LINK: &str = ".base16_theme.fish";

pub struct Profile {
    theme_link: PathBuf,
    fish_link: PathBuf,
    hooks: Option<PathBuf>,
}

impl Profile {
    pub fn new(home: &Path, hooks: Option<OsString>) -> Self {
        Self {
            theme_link: home.join(THEME_LINK),
            fish_link: home.join(FISH_THEME_LINK),
            hooks: hooks.map(PathBuf::from),
        }
    }

    pub fn enabled_script(&self) -> Result<Option<PathBuf>> {
        if !self.theme_link.exists() {
            return Ok(None);
        }
        fs::canonicalize(&self.theme_link)
            .map(Some)
            .with_context(|| format!("resolving the link {}", self.theme_link.display()))
    }

    pub fn enabled(&self) -> Result<Option<Theme>> {
        Ok(self.enabled_script()?.map(Theme::new))
    }

    pub fn enable(&self, theme: &Theme) -> Result<()> {
        replace_link(&self.theme_link, Some(theme.script()))?;
        replace_link(&self.fish_link, theme.fish_script().as_deref())?;
        if let Some(hooks) = &self.hooks {
            run_hooks(hooks, theme)?;
        }
        Ok(())
    }
}

fn replace_link(link: &Path, target: Option<&Path>) -> Result<()> {
    match fs::remove_file(link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("removing {}", link.display())),
    }
    if let Some(target) = target {
        symlink(target, link).with_context(|| format!("linking {}", link.display()))?;
    }
    Ok(())
}

fn run_hooks(dir: &Path, theme: &Theme) -> Result<()> {
    let read_error = || format!("reading the directory {}", dir.display());
    let mut hooks = fs::read_dir(dir)
        .with_context(read_error)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()
        .with_context(read_error)?;
    hooks.retain(|hook| hook.is_file());
    hooks.sort();
    for hook in hooks {
        let status = Command::new(&hook)
            .env("BASE16_THEME", theme.name())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("running {}", hook.display()))?;
        if !status.success() {
            bail!("{} failed: {status}", hook.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn enable_links_the_script_and_fish_variant_and_runs_hooks() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("repo");
        fs::create_dir_all(repo.join("scripts")).unwrap();
        fs::create_dir_all(repo.join("fish")).unwrap();
        fs::write(repo.join("fish").join("base16-a.fish"), "").unwrap();
        let hooks = home.path().join("hooks");
        fs::create_dir(&hooks).unwrap();
        let hook = hooks.join("record");
        let recorded = home.path().join("recorded");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\necho \"$BASE16_THEME\" > '{}'\n",
                recorded.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let profile = Profile::new(home.path(), Some(hooks.into()));

        let a = Theme::new(repo.join("scripts").join("base16-a.sh"));
        let b = Theme::new(repo.join("scripts").join("base16-b.sh"));
        fs::write(a.script(), "").unwrap();
        fs::write(b.script(), "").unwrap();
        assert_eq!(profile.enabled().unwrap(), None);

        profile.enable(&a).unwrap();
        assert_eq!(fs::read_link(&profile.theme_link).unwrap(), a.script());
        assert_eq!(
            fs::read_link(&profile.fish_link).unwrap(),
            repo.join("fish").join("base16-a.fish")
        );
        assert_eq!(fs::read_to_string(&recorded).unwrap(), "a\n");
        assert_eq!(profile.enabled().unwrap().unwrap().name(), "a");

        profile.enable(&b).unwrap();
        assert_eq!(fs::read_link(&profile.theme_link).unwrap(), b.script());
        assert!(!profile.fish_link.is_symlink());
        assert_eq!(fs::read_to_string(&recorded).unwrap(), "b\n");
    }
}
