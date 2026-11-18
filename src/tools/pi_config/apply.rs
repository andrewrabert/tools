//! `apply`: make Pi's agent directory match a spec directory.
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{ExitCode, ExitStatus};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::tools::pi_config::spec::{Config, HostName};
use crate::tools::pi_config::{write, write_json};

const AGENTS_FILENAME: &str = "AGENTS.md";
/// Copied when the spec directory has them, removed from the agent directory when it does not.
const OPTIONAL_FILENAMES: [&str; 2] = ["SYSTEM.md", "APPEND_SYSTEM.md"];
const MODELS_FILENAME: &str = "models.json";
const SETTINGS_FILENAME: &str = "settings.json";

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn remove(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            bail!("refusing to remove directory: {}", path.display())
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    }
    fs::remove_file(path).with_context(|| format!("removing {}", path.display()))
}

struct Agents {
    shared: String,
    host: Option<String>,
}

impl Agents {
    fn read(directory: &Path, host: &HostName) -> Result<Self> {
        let path = directory.join(AGENTS_FILENAME);
        let shared = match fs::read_to_string(&path) {
            Ok(shared) => shared,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                bail!("file not found: {}", path.display())
            }
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let host_path = directory
            .join("hosts")
            .join(host.to_string())
            .join(AGENTS_FILENAME);
        let host = match host_path.is_file() {
            true => Some(
                fs::read_to_string(&host_path)
                    .with_context(|| format!("reading {}", host_path.display()))?,
            ),
            false => None,
        };
        Ok(Self { shared, host })
    }

    fn text(&self) -> String {
        match &self.host {
            Some(host) => format!(
                "{}\n\n{}",
                self.shared.trim_end_matches('\n'),
                host.trim_start_matches('\n')
            ),
            None => self.shared.clone(),
        }
    }
}

/// `install` runs `pi install` for one package source.
pub fn run(
    directory: &Path,
    host: &HostName,
    agent_dir: &Path,
    install: &mut dyn FnMut(&str) -> Result<ExitStatus>,
) -> Result<ExitCode> {
    let config = Config::load(directory, host)?;
    let sources = config.package_sources()?;

    if agent_dir.is_symlink() {
        bail!("refusing to use directory symlink: {}", agent_dir.display());
    }
    fs::create_dir_all(agent_dir).with_context(|| format!("creating {}", agent_dir.display()))?;

    let agents = Agents::read(directory, host)?;
    write(&agent_dir.join(AGENTS_FILENAME), agents.text().as_bytes())?;
    for name in OPTIONAL_FILENAMES {
        match read_optional(&directory.join(name))? {
            Some(data) => write(&agent_dir.join(name), &data)?,
            None => remove(&agent_dir.join(name))?,
        }
    }
    match &config.models {
        Some(models) => write_json(&agent_dir.join(MODELS_FILENAME), models)?,
        None => remove(&agent_dir.join(MODELS_FILENAME))?,
    }

    let settings_path = agent_dir.join(SETTINGS_FILENAME);
    if !sources.is_empty() {
        // Pi must install a package's resources before it can resolve settings that refer to
        // them, such as a package-provided theme.
        let mut bootstrap = config.settings.clone();
        bootstrap.shift_remove("theme");
        bootstrap.insert("packages".to_owned(), Value::Array(Vec::new()));
        write_json(&settings_path, &bootstrap)?;
        for source in sources {
            println!("Installing Pi package {source}");
            let status = install(source)?;
            if !status.success() {
                let code = status.code().and_then(|code| u8::try_from(code).ok());
                return Ok(ExitCode::from(code.unwrap_or(1)));
            }
        }
    }
    write_json(&settings_path, &config.settings)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn host() -> HostName {
        HostName::new("box".to_owned()).unwrap()
    }

    fn tree(files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        dir
    }

    fn json_file(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn no_install(_: &str) -> Result<ExitStatus> {
        panic!("nothing to install");
    }

    #[test]
    fn host_instructions_follow_the_shared_ones_after_one_blank_line() {
        let agents = |shared: &str, host: Option<&str>| Agents {
            shared: shared.to_owned(),
            host: host.map(str::to_owned),
        };
        assert_eq!(agents("a\n\n\n", Some("\n\nb\n")).text(), "a\n\nb\n");
        assert_eq!(agents("a", Some("b")).text(), "a\n\nb");
        assert_eq!(agents("\na\n\n", None).text(), "\na\n\n");
    }

    #[test]
    fn writes_files_and_removes_the_ones_the_spec_lacks() {
        let spec = tree(&[
            ("pi-config.yaml", "settings: {b: 1, a: é}\n"),
            ("AGENTS.md", "main\n\n"),
            ("hosts/box/AGENTS.md", "\nhost\n"),
            ("SYSTEM.md", "system\n"),
        ]);
        let home = tree(&[
            ("agent/APPEND_SYSTEM.md", "stale"),
            ("agent/models.json", "{}"),
            ("agent/settings.json", "{}"),
        ]);
        let agent = home.path().join("agent");
        fs::set_permissions(
            agent.join("settings.json"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();

        let code = run(spec.path(), &host(), &agent, &mut no_install).unwrap();

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(
            fs::read_to_string(agent.join("AGENTS.md")).unwrap(),
            "main\n\nhost\n"
        );
        assert_eq!(
            fs::read_to_string(agent.join("SYSTEM.md")).unwrap(),
            "system\n"
        );
        assert!(!agent.join("APPEND_SYSTEM.md").exists());
        assert!(!agent.join("models.json").exists());
        assert_eq!(
            fs::read_to_string(agent.join("settings.json")).unwrap(),
            "{\n  \"b\": 1,\n  \"a\": \"é\"\n}\n"
        );
        let mode =
            |name: &str| fs::metadata(agent.join(name)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode("settings.json"), 0o640);
        assert_eq!(mode("AGENTS.md"), 0o600);
    }

    #[test]
    fn packages_install_against_settings_without_them_or_the_theme() {
        let spec = tree(&[
            (
                "pi-config.yaml",
                "packages: [one, two]\nsettings: {theme: fancy, a: 1}\nmodels: {providers: {}}\n",
            ),
            ("AGENTS.md", "main\n"),
            ("packages/one.yaml", "source: npm:one\n"),
            ("packages/two.yaml", "source: git:two\ndisabled: [x]\n"),
        ]);
        let home = tempfile::tempdir().unwrap();
        let agent = home.path().join(".pi/agent");
        let settings = agent.join("settings.json");
        let mut installed = Vec::new();
        let mut install = |source: &str| {
            assert_eq!(json_file(&settings), json!({"a": 1, "packages": []}));
            installed.push(source.to_owned());
            Ok(ExitStatus::default())
        };

        let code = run(spec.path(), &host(), &agent, &mut install).unwrap();

        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(installed, ["npm:one", "git:two"]);
        assert_eq!(
            json_file(&settings),
            json!({"theme": "fancy", "a": 1, "packages": [
                "npm:one",
                {"source": "git:two", "extensions": ["extensions/**", "!extensions/x/**"]},
            ]})
        );
        assert_eq!(
            json_file(&agent.join("models.json")),
            json!({"providers": {}})
        );
    }

    #[test]
    fn a_failed_install_stops_with_its_exit_code() {
        let spec = tree(&[
            ("pi-config.yaml", "settings: {packages: [a, b]}\n"),
            ("AGENTS.md", "main\n"),
        ]);
        let home = tempfile::tempdir().unwrap();
        let agent = home.path().join("agent");
        let mut calls = 0;
        let mut install = |_: &str| {
            calls += 1;
            Ok(ExitStatus::from_raw(3 << 8))
        };

        let code = run(spec.path(), &host(), &agent, &mut install).unwrap();

        assert_eq!(code, ExitCode::from(3));
        assert_eq!(calls, 1);
        assert_eq!(
            json_file(&agent.join("settings.json")),
            json!({"packages": []})
        );
    }

    #[test]
    fn refuses_what_it_would_have_to_destroy() {
        let spec = tree(&[("pi-config.yaml", ""), ("AGENTS.md", "main\n")]);
        let home = tree(&[("agent/models.json/keep", "")]);
        let error = run(
            spec.path(),
            &host(),
            &home.path().join("agent"),
            &mut no_install,
        );
        assert!(format!("{:#}", error.unwrap_err()).contains("refusing to remove directory"));

        std::os::unix::fs::symlink(home.path().join("agent"), home.path().join("link")).unwrap();
        let error = run(
            spec.path(),
            &host(),
            &home.path().join("link"),
            &mut no_install,
        );
        assert!(format!("{:#}", error.unwrap_err()).contains("refusing to use directory symlink"));

        let bare = tree(&[("pi-config.yaml", "")]);
        let error = run(
            bare.path(),
            &host(),
            &home.path().join("other"),
            &mut no_install,
        );
        assert!(format!("{:#}", error.unwrap_err()).contains("file not found"));
    }
}
