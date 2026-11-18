//! The spec directory: `pi-config.yaml` and its `hosts/`, `providers/` and `packages/` files.
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::yaml;

pub const SPEC_FILENAME: &str = "pi-config.yaml";
const HOSTS_DIRNAME: &str = "hosts";
const PACKAGES_DIRNAME: &str = "packages";
const PROVIDERS_DIRNAME: &str = "providers";

/// A file or folder name that is safe to place in a path or a glob as written.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String")]
struct Name(String);

impl TryFrom<String> for Name {
    type Error = anyhow::Error;

    fn try_from(name: String) -> Result<Self> {
        let mut characters = name.chars();
        let valid = characters
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
            && characters.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            bail!(
                "name must start with a letter or digit and hold only letters, digits, _ and -: \
                 {name:?}"
            );
        }
        Ok(Self(name))
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name a host's files in `hosts/` carry: one path component, so it cannot lead outside `hosts/`.
#[derive(Debug)]
pub struct HostName(String);

impl HostName {
    pub fn new(name: String) -> Result<Self> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
            bail!("host must be a single path component: {name:?}");
        }
        Ok(Self(name))
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `pi-config.yaml`, and each provider file layered over it.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Spec {
    /// Names of files in `packages/`. Only the main spec's are used.
    #[serde(default)]
    packages: Vec<Name>,
    #[serde(default)]
    settings: Map<String, Value>,
    #[serde(default)]
    models: Option<Map<String, Value>>,
}

impl Spec {
    fn merge(&mut self, provider: Spec) {
        merge_objects(&mut self.settings, provider.settings);
        if let Some(models) = provider.models {
            merge_objects(self.models.get_or_insert_default(), models);
        }
    }
}

/// Which of a package's extensions load: all of them unless `enabled` names some.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Selection {
    #[serde(default)]
    enabled: Option<Vec<Name>>,
    #[serde(default)]
    disabled: Vec<Name>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PackageSpec {
    source: String,
    #[serde(default)]
    enabled: Option<Vec<Name>>,
    #[serde(default)]
    disabled: Vec<Name>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct HostSpec {
    /// Names of files in `providers/`. The first one supplies the default provider and model.
    providers: Vec<String>,
    #[serde(default)]
    packages: BTreeMap<Name, Selection>,
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("spec file not found: {}", path.display())
        }
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    parse(&text).with_context(|| path.display().to_string())
}

fn parse<T: DeserializeOwned>(text: &str) -> Result<T> {
    let value = yaml::load(text).context("invalid YAML")?;
    if !value.is_object() {
        bail!("expected a YAML mapping");
    }
    Ok(serde_json::from_value(value)?)
}

fn merge_objects(base: &mut Map<String, Value>, overrides: Map<String, Value>) {
    for (key, value) in overrides {
        match (base.get_mut(&key), value) {
            (Some(Value::Object(current)), Value::Object(value)) => merge_objects(current, value),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

fn has_extension(reference: &str) -> bool {
    let name = reference.rsplit('/').next().unwrap_or(reference);
    name.rfind('.')
        .is_some_and(|dot| dot > 0 && dot < name.len() - 1)
}

fn read_provider(config_dir: &Path, reference: &str) -> Result<Spec> {
    let relative = reference
        .trim_end_matches('/')
        .split('/')
        .all(|part| !part.is_empty() && part != "..")
        && Path::new(reference)
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if !relative || has_extension(reference) {
        bail!("provider reference must be relative and omit .yaml: {reference:?}");
    }
    let providers_dir = config_dir.join(PROVIDERS_DIRNAME);
    let path = providers_dir.join(format!("{reference}.yaml"));
    // A file that is missing has nowhere to escape to; reading it reports it as not found.
    if let Ok(resolved) = fs::canonicalize(&path) {
        let providers_dir = fs::canonicalize(&providers_dir)
            .with_context(|| format!("resolving {}", providers_dir.display()))?;
        if !resolved.starts_with(&providers_dir) {
            bail!("provider reference escapes providers directory: {reference:?}");
        }
    }
    read(&path)
}

#[derive(Debug, PartialEq)]
struct DefaultModel {
    provider: String,
    model: String,
}

impl DefaultModel {
    /// An `enabledModels` entry, written `provider/model`; the model may itself hold a `/`.
    fn parse(entry: &str) -> Option<Self> {
        let (provider, model) = entry.split_once('/')?;
        Some(Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
    }

    fn of_provider(reference: &str, spec: &Spec) -> Result<Self> {
        let enabled = spec
            .settings
            .get("enabledModels")
            .and_then(|models| models.get(0))
            .and_then(Value::as_str)
            .and_then(Self::parse);
        if let Some(enabled) = enabled {
            return Ok(enabled);
        }
        let first = spec
            .models
            .as_ref()
            .and_then(|models| models.get("providers"))
            .and_then(Value::as_object)
            .and_then(|providers| providers.iter().next());
        if let Some((provider, definition)) = first
            && let Some(model) = definition
                .get("models")
                .and_then(|models| models.get(0))
                .and_then(|model| model.get("id"))
                .and_then(Value::as_str)
        {
            return Ok(Self {
                provider: provider.clone(),
                model: model.to_owned(),
            });
        }
        bail!(
            "cannot determine default provider and model from {PROVIDERS_DIRNAME}/{reference}.yaml"
        );
    }

    fn set_in(self, settings: &mut Map<String, Value>) {
        settings.insert("defaultProvider".to_owned(), Value::String(self.provider));
        settings.insert("defaultModel".to_owned(), Value::String(self.model));
    }
}

/// The `settings.packages` entry of a package: its bare source when every extension loads.
fn package_entry(
    reference: &Name,
    package: PackageSpec,
    host: Option<&Selection>,
) -> Result<Value> {
    if package.source.is_empty() {
        bail!("package {reference} has an empty source");
    }
    let mut enabled = package.enabled;
    let mut disabled = package.disabled;
    if let Some(host) = host {
        if host.enabled.is_some() {
            enabled.clone_from(&host.enabled);
        }
        disabled.extend(host.disabled.iter().cloned());
    }
    let mut unique: Vec<Name> = Vec::new();
    for name in disabled {
        if !unique.contains(&name) {
            unique.push(name);
        }
    }

    if enabled.is_none() && unique.is_empty() {
        return Ok(Value::String(package.source));
    }
    let mut extensions = match enabled {
        Some(enabled) => enabled
            .iter()
            .map(|name| format!("extensions/{name}/**"))
            .collect(),
        None => vec!["extensions/**".to_owned()],
    };
    extensions.extend(unique.iter().map(|name| format!("!extensions/{name}/**")));
    Ok(json!({"source": package.source, "extensions": extensions}))
}

fn resolve_packages(
    config_dir: &Path,
    references: &[Name],
    host: &BTreeMap<Name, Selection>,
) -> Result<Vec<Value>> {
    let unknown: Vec<String> = host
        .keys()
        .filter(|name| !references.contains(name))
        .map(Name::to_string)
        .collect();
    if !unknown.is_empty() {
        bail!(
            "host overrides packages not referenced by main spec: {}",
            unknown.join(", ")
        );
    }
    references
        .iter()
        .map(|reference| {
            let path = config_dir
                .join(PACKAGES_DIRNAME)
                .join(format!("{reference}.yaml"));
            package_entry(reference, read(&path)?, host.get(reference))
        })
        .collect()
}

fn package_source(entry: &Value) -> Result<&str> {
    let source = match entry {
        Value::String(source) => Some(source.as_str()),
        entry => entry.get("source").and_then(Value::as_str),
    };
    source.with_context(|| format!("invalid package entry: {entry}"))
}

/// What a spec directory resolves to for one host: the contents of `settings.json` and `models.json`.
#[derive(Debug)]
pub struct Config {
    pub settings: Map<String, Value>,
    pub models: Option<Map<String, Value>>,
}

impl Config {
    pub fn load(config_dir: &Path, host: &HostName) -> Result<Self> {
        let mut spec: Spec = read(&config_dir.join(SPEC_FILENAME))?;
        let host_path = config_dir.join(HOSTS_DIRNAME).join(format!("{host}.yaml"));
        let mut host_packages = BTreeMap::new();
        if host_path.is_file() {
            let host: HostSpec = read(&host_path)?;
            let Some(first) = host.providers.first() else {
                bail!("{}: providers must name a provider", host_path.display());
            };
            let mut defaults = None;
            for reference in &host.providers {
                let provider = read_provider(config_dir, reference)?;
                if defaults.is_none() {
                    defaults = Some(DefaultModel::of_provider(first, &provider)?);
                }
                spec.merge(provider);
            }
            if let Some(defaults) = defaults {
                defaults.set_in(&mut spec.settings);
            }
            host_packages = host.packages;
        }
        if !spec.packages.is_empty() {
            let packages = resolve_packages(config_dir, &spec.packages, &host_packages)?;
            spec.settings
                .insert("packages".to_owned(), Value::Array(packages));
        }
        let config = Self {
            settings: spec.settings,
            models: spec.models,
        };
        config.package_sources()?;
        Ok(config)
    }

    pub fn package_sources(&self) -> Result<Vec<&str>> {
        match self.settings.get("packages") {
            None => Ok(Vec::new()),
            Some(Value::Array(packages)) => packages.iter().map(package_source).collect(),
            Some(_) => bail!("settings.packages must be an array"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    fn spec_dir(files: &[(&str, &str)]) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        dir
    }

    fn host() -> HostName {
        HostName::new("box".to_owned()).unwrap()
    }

    #[test]
    fn host_names_are_single_path_components() {
        for name in ["", ".", "..", "a/b", "/a"] {
            assert!(HostName::new(name.to_owned()).is_err(), "{name}");
        }
        assert_eq!(
            HostName::new("a.example".to_owned()).unwrap().to_string(),
            "a.example"
        );
    }

    fn load(files: &[(&str, &str)]) -> Result<Config> {
        Config::load(spec_dir(files).path(), &host())
    }

    fn error(files: &[(&str, &str)]) -> String {
        format!("{:#}", load(files).unwrap_err())
    }

    #[test]
    fn main_spec_alone_passes_through() {
        let config = load(&[(SPEC_FILENAME, "settings:\n  theme: dark\n")]).unwrap();
        assert_eq!(Value::Object(config.settings), json!({"theme": "dark"}));
        assert_eq!(config.models, None);
        assert_eq!(load(&[(SPEC_FILENAME, "")]).unwrap().settings, Map::new());
    }

    #[test]
    fn spec_must_be_a_mapping_of_known_keys() {
        assert!(error(&[]).contains("spec file not found"));
        assert!(error(&[(SPEC_FILENAME, "- a\n")]).contains("expected a YAML mapping"));
        assert!(error(&[(SPEC_FILENAME, "extra: 1\n")]).contains("unknown field"));
        assert!(error(&[(SPEC_FILENAME, "a: [\n")]).contains("invalid YAML"));
    }

    #[test]
    fn providers_merge_in_order_and_the_first_sets_the_defaults() {
        let config = load(&[
            (
                SPEC_FILENAME,
                "settings:\n  theme: dark\n  nested: {a: 1, b: 2}\n",
            ),
            ("hosts/box.yaml", "providers: [one, sub/two]\n"),
            (
                "providers/one.yaml",
                "settings:\n  enabledModels: [p1/m/x, p1/n]\n  nested: {b: 3}\n\
                 models:\n  providers:\n    p1: {models: [{id: m}]}\n",
            ),
            (
                "providers/sub/two.yaml",
                "settings:\n  enabledModels: [p2/z]\nmodels:\n  providers:\n    p2: {baseUrl: u}\n",
            ),
        ])
        .unwrap();
        assert_eq!(
            serde_json::to_string(&config.settings).unwrap(),
            r#"{"theme":"dark","nested":{"a":1,"b":3},"enabledModels":["p2/z"],"defaultProvider":"p1","defaultModel":"m/x"}"#
        );
        assert_eq!(
            serde_json::to_string(&config.models).unwrap(),
            r#"{"providers":{"p1":{"models":[{"id":"m"}]},"p2":{"baseUrl":"u"}}}"#
        );
    }

    #[test]
    fn defaults_fall_back_to_the_first_model_of_the_first_provider() {
        let host = ("hosts/box.yaml", "providers: [one]\n");
        let config = load(&[
            (SPEC_FILENAME, ""),
            host,
            (
                "providers/one.yaml",
                "models:\n  providers:\n    zed: {models: [{id: first}, {id: second}]}\n    abc: {}\n",
            ),
        ])
        .unwrap();
        assert_eq!(
            Value::Object(config.settings),
            json!({"defaultProvider": "zed", "defaultModel": "first"})
        );
        let message = error(&[(SPEC_FILENAME, ""), host, ("providers/one.yaml", "")]);
        assert!(message.contains("cannot determine default provider and model"));
    }

    #[test]
    fn another_hosts_file_is_ignored() {
        let config = load(&[
            (SPEC_FILENAME, "settings: {a: 1}\n"),
            ("hosts/other.yaml", "providers: [missing]\n"),
        ])
        .unwrap();
        assert_eq!(Value::Object(config.settings), json!({"a": 1}));
    }

    #[test]
    fn host_must_name_a_provider() {
        let message = error(&[(SPEC_FILENAME, ""), ("hosts/box.yaml", "providers: []\n")]);
        assert!(message.contains("providers must name a provider"));
    }

    #[test]
    fn provider_references_stay_inside_the_providers_directory() {
        for reference in ["../one", "/one", "one.yaml", "a//b", "''"] {
            let host = format!("providers: [{reference}]\n");
            let message = error(&[(SPEC_FILENAME, ""), ("hosts/box.yaml", &host)]);
            assert!(
                message.contains("must be relative and omit .yaml"),
                "{reference}: {message}"
            );
        }
        let dir = spec_dir(&[
            (SPEC_FILENAME, ""),
            ("hosts/box.yaml", "providers: [link]\n"),
            ("outside.yaml", "settings: {enabledModels: [p/m]}\n"),
            ("providers/.keep", ""),
        ]);
        symlink(
            dir.path().join("outside.yaml"),
            dir.path().join("providers/link.yaml"),
        )
        .unwrap();
        let message = format!("{:#}", Config::load(dir.path(), &host()).unwrap_err());
        assert!(message.contains("escapes providers directory"));
        let message = error(&[
            (SPEC_FILENAME, ""),
            ("hosts/box.yaml", "providers: [gone]\n"),
        ]);
        assert!(message.contains("spec file not found"));
    }

    #[test]
    fn packages_resolve_to_sources_and_extension_filters() {
        let config = load(&[
            (SPEC_FILENAME, "packages: [plain, some, most]\n"),
            (
                "hosts/box.yaml",
                "providers: [one]\npackages:\n  most: {disabled: [b, c]}\n  some: {enabled: [w]}\n",
            ),
            ("providers/one.yaml", "settings: {enabledModels: [p/m]}\n"),
            ("packages/plain.yaml", "source: npm:plain\n"),
            ("packages/some.yaml", "source: git:some\nenabled: [u, v]\n"),
            ("packages/most.yaml", "source: git:most\ndisabled: [a, b]\n"),
        ])
        .unwrap();
        assert_eq!(
            config.settings["packages"],
            json!([
                "npm:plain",
                {"source": "git:some", "extensions": ["extensions/w/**"]},
                {"source": "git:most", "extensions": [
                    "extensions/**", "!extensions/a/**", "!extensions/b/**", "!extensions/c/**"
                ]},
            ])
        );
        assert_eq!(
            config.package_sources().unwrap(),
            ["npm:plain", "git:some", "git:most"]
        );
    }

    #[test]
    fn package_mistakes_are_errors() {
        let main = (SPEC_FILENAME, "packages: [one]\n");
        let provider = ("providers/p.yaml", "settings: {enabledModels: [p/m]}\n");
        let message = error(&[
            main,
            ("hosts/box.yaml", "providers: [p]\npackages: {two: {}}\n"),
            provider,
        ]);
        assert!(
            message.contains("not referenced by main spec: two"),
            "{message}"
        );
        let message = error(&[main, ("packages/one.yaml", "source: s\nenabled: [a/b]\n")]);
        assert!(
            message.contains("name must start with a letter or digit"),
            "{message}"
        );
        let message = error(&[main, ("packages/one.yaml", "source: ''\n")]);
        assert!(message.contains("empty source"), "{message}");
        let message = error(&[(SPEC_FILENAME, "packages: [../one]\n")]);
        assert!(
            message.contains("name must start with a letter or digit"),
            "{message}"
        );
    }

    #[test]
    fn packages_written_in_settings_are_checked() {
        let config = load(&[(SPEC_FILENAME, "settings:\n  packages: [a, {source: b}]\n")]).unwrap();
        assert_eq!(config.package_sources().unwrap(), ["a", "b"]);
        let message = error(&[(SPEC_FILENAME, "settings:\n  packages: [{name: b}]\n")]);
        assert!(message.contains("invalid package entry"), "{message}");
        let message = error(&[(SPEC_FILENAME, "settings:\n  packages: a\n")]);
        assert!(
            message.contains("settings.packages must be an array"),
            "{message}"
        );
    }
}
