use std::ffi::OsStr;
use std::fs;
use std::path::{self, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::tools::base16_shell_preview::profile::Profile;

const NAME_PREFIX: &str = "base16-";
const SHELL: &str = "/bin/sh";

pub struct Repo {
    root: PathBuf,
}

impl Repo {
    pub fn locate(r#override: Option<&OsStr>, profile: &Profile) -> Result<Self> {
        if let Some(root) = r#override {
            return Ok(Self { root: root.into() });
        }
        if let Some(script) = profile.enabled_script()?
            && let Some(root) = script.parent().and_then(Path::parent)
        {
            return Ok(Self { root: root.into() });
        }
        bail!("set the BASE16_SHELL environment variable to the Base16 Shell repository path")
    }

    pub fn themes(&self, order: Order) -> Result<Vec<Theme>> {
        let scripts = self.root.join("scripts");
        let read_error = || format!("reading the directory {}", scripts.display());
        let mut themes = Vec::new();
        for entry in fs::read_dir(&scripts).with_context(read_error)? {
            let path = entry.with_context(read_error)?.path();
            if path.is_file() {
                let path = path::absolute(&path)
                    .with_context(|| format!("resolving the path {}", path.display()))?;
                themes.push(Theme::new(path));
            }
        }
        match order {
            Order::Name => themes.sort_by(|a, b| a.name.cmp(&b.name)),
            Order::Background => {
                let mut keyed = themes
                    .into_iter()
                    .map(|theme| Ok((theme.background()?, theme)))
                    .collect::<Result<Vec<_>>>()?;
                keyed.sort_by(|(a_bg, a), (b_bg, b)| {
                    a_bg.cmp(b_bg).then_with(|| a.name.cmp(&b.name))
                });
                themes = keyed.into_iter().map(|(_, theme)| theme).collect();
            }
        }
        Ok(themes)
    }
}

#[derive(Clone, Copy)]
pub enum Order {
    Name,
    Background,
}

#[derive(Debug, PartialEq)]
pub struct Theme {
    name: String,
    script: PathBuf,
}

impl Theme {
    pub fn new(script: PathBuf) -> Self {
        let stem = script
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = stem.strip_prefix(NAME_PREFIX).unwrap_or(&stem).to_owned();
        Self { name, script }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn script(&self) -> &Path {
        &self.script
    }

    pub fn fish_script(&self) -> Option<PathBuf> {
        let root = self.script.parent()?.parent()?;
        let mut file = self.script.file_stem()?.to_owned();
        file.push(".fish");
        Some(root.join("fish").join(file)).filter(|fish| fish.is_file())
    }

    pub fn apply(&self) -> Result<()> {
        let status = Command::new(SHELL)
            .arg(&self.script)
            .status()
            .with_context(|| format!("running {SHELL}"))?;
        if !status.success() {
            bail!("{} failed: {status}", self.script.display());
        }
        Ok(())
    }

    fn background(&self) -> Result<Rgb> {
        let script = fs::read_to_string(&self.script)
            .with_context(|| format!("reading {}", self.script.display()))?;
        Rgb::background(&script).with_context(|| format!("no color00 in {}", self.script.display()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Rgb(u32);

impl Rgb {
    fn background(script: &str) -> Option<Self> {
        let line = script.lines().find(|line| line.starts_with("color00"))?;
        let value = line.split('"').nth(1)?.replace('/', "");
        u32::from_str_radix(&value, 16).ok().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn names(themes: &[Theme]) -> Vec<&str> {
        themes.iter().map(Theme::name).collect()
    }

    #[test]
    fn theme_name_drops_prefix_and_extension() {
        assert_eq!(
            Theme::new("/s/scripts/base16-ocean.sh".into()).name(),
            "ocean"
        );
        assert_eq!(Theme::new("/s/scripts/other.sh".into()).name(), "other");
    }

    #[test]
    fn background_is_read_from_color00() {
        let script = "#!/bin/sh\ncolor00=\"2b/30/3b\" # Base 00 - Black\ncolor01=\"bf/61/6a\"\n";
        assert_eq!(Rgb::background(script), Some(Rgb(0x2b303b)));
        assert_eq!(Rgb::background("color01=\"00/00/00\"\n"), None);
    }

    #[test]
    fn locate_prefers_the_override() {
        let profile = Profile::new(Path::new("/nonexistent"), None);
        let repo = Repo::locate(Some(OsStr::new("/repo")), &profile).unwrap();
        assert_eq!(repo.root, PathBuf::from("/repo"));
    }

    #[test]
    fn locate_follows_the_enabled_theme() {
        let home = tempfile::tempdir().unwrap();
        let scripts = home.path().join("repo").join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("base16-a.sh"), "").unwrap();
        let profile = Profile::new(home.path(), None);
        assert!(Repo::locate(None, &profile).is_err());

        symlink(
            scripts.join("base16-a.sh"),
            home.path().join(".base16_theme"),
        )
        .unwrap();
        let repo = Repo::locate(None, &profile).unwrap();
        assert_eq!(
            repo.root,
            fs::canonicalize(home.path().join("repo")).unwrap()
        );
    }

    #[test]
    fn themes_sort_by_name_or_background() {
        let root = tempfile::tempdir().unwrap();
        let scripts = root.path().join("scripts");
        fs::create_dir(&scripts).unwrap();
        fs::write(scripts.join("base16-a.sh"), "color00=\"ff/ff/ff\"\n").unwrap();
        fs::write(scripts.join("base16-b.sh"), "color00=\"00/00/00\"\n").unwrap();
        fs::write(scripts.join("base16-c.sh"), "color00=\"00/00/00\"\n").unwrap();
        let repo = Repo {
            root: root.path().into(),
        };
        assert_eq!(names(&repo.themes(Order::Name).unwrap()), ["a", "b", "c"]);
        assert_eq!(
            names(&repo.themes(Order::Background).unwrap()),
            ["b", "c", "a"]
        );
    }

    #[test]
    fn fish_script_is_the_sibling_variant() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("fish")).unwrap();
        fs::write(root.path().join("fish").join("base16-a.fish"), "").unwrap();
        let scripts = root.path().join("scripts");
        assert_eq!(
            Theme::new(scripts.join("base16-a.sh")).fish_script(),
            Some(root.path().join("fish").join("base16-a.fish"))
        );
        assert_eq!(Theme::new(scripts.join("base16-b.sh")).fish_script(), None);
    }
}
