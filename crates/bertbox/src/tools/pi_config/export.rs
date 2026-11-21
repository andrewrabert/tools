//! `export`: write Pi's current settings and models as a spec.
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::tools::pi_config::spec::SPEC_FILENAME;
use crate::tools::pi_config::write;
use crate::yaml;

#[derive(Serialize)]
struct Export {
    settings: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    models: Option<Map<String, Value>>,
}

fn read_object(path: &Path) -> Result<Option<Map<String, Value>>> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    serde_json::from_slice(&data)
        .map(Some)
        .with_context(|| format!("invalid JSON object in {}", path.display()))
}

pub fn run(directory: &Path, force: bool, agent_dir: &Path) -> Result<()> {
    let output = directory.join(SPEC_FILENAME);
    if output.exists() && !force {
        bail!("path exists (use -f to overwrite): {}", output.display());
    }
    let settings_path = agent_dir.join("settings.json");
    let Some(settings) = read_object(&settings_path)? else {
        bail!("file not found: {}", settings_path.display());
    };
    let models = read_object(&agent_dir.join("models.json"))?;

    fs::create_dir_all(directory).with_context(|| format!("creating {}", directory.display()))?;
    write(
        &output,
        yaml::dump(&Export { settings, models })?.as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::pi_config::spec::{Config, HostName};

    #[test]
    fn exported_spec_loads_back_to_the_same_settings_and_models() {
        let home = tempfile::tempdir().unwrap();
        let settings = r#"{"theme": "dark", "enabledModels": ["p/m"], "num": 1.5}"#;
        let models = r#"{"providers": {"p": {"models": [{"id": "m"}]}}}"#;
        fs::write(home.path().join("settings.json"), settings).unwrap();
        let spec = home.path().join("spec/nested");

        run(&spec, false, home.path()).unwrap();
        assert_eq!(
            fs::read_to_string(spec.join(SPEC_FILENAME)).unwrap(),
            "settings:\n  theme: dark\n  enabledModels:\n  - p/m\n  num: 1.5\n"
        );

        let error = run(&spec, false, home.path()).unwrap_err();
        assert!(format!("{error:#}").contains("path exists (use -f to overwrite)"));

        fs::write(home.path().join("models.json"), models).unwrap();
        run(&spec, true, home.path()).unwrap();
        let config = Config::load(&spec, &HostName::new("box".to_owned()).unwrap()).unwrap();
        assert_eq!(
            Value::Object(config.settings),
            serde_json::from_str::<Value>(settings).unwrap()
        );
        assert_eq!(config.models, serde_json::from_str(models).unwrap());
    }

    #[test]
    fn settings_must_exist_and_hold_an_object() {
        let home = tempfile::tempdir().unwrap();
        let spec = home.path().join("spec");
        let error = run(&spec, false, home.path()).unwrap_err();
        assert!(format!("{error:#}").contains("file not found"));
        fs::write(home.path().join("settings.json"), "[]").unwrap();
        let error = run(&spec, false, home.path()).unwrap_err();
        assert!(format!("{error:#}").contains("invalid JSON object"));
        assert!(!spec.exists());
    }
}
