//! Claude's own files: `.claude.json`, `settings.json` and the plugin registry.
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::dirs;
use crate::tools::claude_config::files::{Log, read_optional, write_if_different};
use crate::tools::claude_config::paths::{expand_user, normalize_source, resolve};
use crate::tools::claude_config::spec::{ConfigSpec, HIDDEN_MARKETPLACE_NAME, PluginRef};
use crate::yaml::sorted;

const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";
const CONFIG_FILENAME: &str = ".claude.json";

/// Claude's trust check walks the working directory's ancestors up to the filesystem root, so the
/// `/` entry trusts everything.
const TRUST_PATH: &str = "/";
const TRUST_KEY: &str = "hasTrustDialogAccepted";
const PROJECTS_KEY: &str = "projects";
const MCP_SERVERS_KEY: &str = "mcpServers";
const ENABLED_PLUGINS_KEY: &str = "enabledPlugins";
const ENV_KEY: &str = "env";
const MODEL_PICKER_KEY: &str = "modelPicker";

/// Claude never fetches flags with telemetry off, so rollout-gated features stay dark; it reads
/// this cache instead when the pinned env var is set. The cache is replaced by these pins.
const GROWTHBOOK_KEY: &str = "cachedGrowthBookFeatures";
/// Fall back to AGENTS.md when CLAUDE.md is not present.
const GROWTHBOOK_PINS: [(&str, bool); 1] = [("tengu_agents_md_mod", true)];
/// Env vars set in `settings.json` on every save.
const PINNED_ENV: [(&str, &str); 1] = [("CLAUDE_CODE_GB_DISK_CACHE_WHEN_TELEMETRY_OFF", "1")];

/// `settings.json` keys Claude manages itself: plugin and marketplace state, and toggles written
/// from its own menus. A spec never authors them; apply keeps them and export leaves them out.
pub const UNMANAGED_SETTINGS: [&str; 11] = [
    "$schema",
    "enabledPlugins",
    "extraKnownMarketplaces",
    "model",
    "advisorModel",
    "enableArtifact",
    "askUserQuestionTimeout",
    "autoUpdatesChannel",
    "preferredNotifChannel",
    "autoScrollEnabled",
    "autoCompactEnabled",
];

fn load_json(path: &Path) -> Result<Map<String, Value>> {
    let Some(data) = read_optional(path)? else {
        return Ok(Map::new());
    };
    serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))
}

pub fn dump_json(value: Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(&sorted(value))? + "\n")
}

/// `.claude.json`. Keys this tool does not manage pass through untouched.
pub struct ClaudeJson {
    data: Map<String, Value>,
}

impl ClaudeJson {
    fn from_map(mut data: Map<String, Value>) -> Result<Self> {
        ConfigSpec::from_claude_json(&data)?;
        let pins: Map<String, Value> = GROWTHBOOK_PINS
            .iter()
            .map(|(flag, value)| ((*flag).to_owned(), Value::Bool(*value)))
            .collect();
        data.insert(GROWTHBOOK_KEY.to_owned(), Value::Object(pins));
        let projects = data
            .entry(PROJECTS_KEY)
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(projects) = projects.as_object() else {
            bail!("{PROJECTS_KEY} is not an object");
        };
        for (path, entry) in projects {
            let Some(entry) = entry.as_object() else {
                bail!("{PROJECTS_KEY}.{path} is not an object");
            };
            if !matches!(
                entry.get(TRUST_KEY),
                None | Some(Value::Null | Value::Bool(_))
            ) {
                bail!("{PROJECTS_KEY}.{path}.{TRUST_KEY} is not a boolean");
            }
        }
        Ok(Self { data })
    }

    fn projects_mut(&mut self) -> &mut Map<String, Value> {
        self.data
            .get_mut(PROJECTS_KEY)
            .and_then(Value::as_object_mut)
            .expect("projects is an object once loaded")
    }

    pub fn config_spec(&self) -> Result<ConfigSpec> {
        let mut config = ConfigSpec::from_claude_json(&self.data)?;
        config.has_trust_dialog_accepted = Some(self.trusts_everything());
        Ok(config)
    }

    pub fn set_keys(&mut self, keys: Map<String, Value>) {
        self.data.extend(keys);
    }

    fn trusts_everything(&self) -> bool {
        let root = self
            .data
            .get(PROJECTS_KEY)
            .map(|projects| &projects[TRUST_PATH]);
        root.is_some_and(|root| root[TRUST_KEY] == Value::Bool(true))
    }

    pub fn trust_everything(&mut self) {
        let projects = self.projects_mut();
        projects
            .entry(TRUST_PATH)
            .or_insert_with(|| Value::Object(Map::new()));
        for entry in projects.values_mut() {
            if let Some(entry) = entry.as_object_mut() {
                entry.insert(TRUST_KEY.to_owned(), Value::Bool(true));
            }
        }
    }

    pub fn mcp_servers(&self) -> Result<Map<String, Value>> {
        match self.data.get(MCP_SERVERS_KEY) {
            None | Some(Value::Null) => Ok(Map::new()),
            Some(Value::Object(servers)) => Ok(servers.clone()),
            Some(_) => bail!("{MCP_SERVERS_KEY} is not an object"),
        }
    }

    pub fn set_mcp_servers(&mut self, servers: Map<String, Value>) {
        if servers.is_empty() {
            self.data.remove(MCP_SERVERS_KEY);
        } else {
            self.data
                .insert(MCP_SERVERS_KEY.to_owned(), Value::Object(servers));
        }
    }

    /// The file's contents. Null values are dropped at the top level and directly inside a
    /// project entry, nowhere deeper.
    fn dump(&self) -> Result<String> {
        let mut data = self.data.clone();
        data.retain(|_, value| !value.is_null());
        if let Some(projects) = data.get_mut(PROJECTS_KEY).and_then(Value::as_object_mut) {
            for entry in projects.values_mut() {
                if let Some(entry) = entry.as_object_mut() {
                    entry.retain(|_, value| !value.is_null());
                }
            }
        }
        dump_json(Value::Object(data))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModelPickerOption {
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    behaves_as: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModelPicker {
    options: Vec<ModelPickerOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replace_built_in_options: Option<bool>,
}

/// `settings.json`, held in the form it is saved in. Keys this tool does not know pass through
/// untouched.
#[derive(Debug, PartialEq)]
pub struct Settings {
    data: Map<String, Value>,
}

impl Settings {
    pub fn from_map(mut data: Map<String, Value>) -> Result<Self> {
        match data.get(ENABLED_PLUGINS_KEY) {
            None => {}
            Some(Value::Object(plugins)) => {
                if let Some((plugin, _)) = plugins.iter().find(|(_, value)| !value.is_boolean()) {
                    bail!("{ENABLED_PLUGINS_KEY}.{plugin} is not a boolean");
                }
                if plugins.is_empty() {
                    data.remove(ENABLED_PLUGINS_KEY);
                }
            }
            Some(_) => bail!("{ENABLED_PLUGINS_KEY} is not an object"),
        }

        let env = data
            .entry(ENV_KEY)
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(env) = env.as_object_mut() else {
            bail!("{ENV_KEY} is not an object");
        };
        for (name, value) in PINNED_ENV {
            env.insert(name.to_owned(), Value::String(value.to_owned()));
        }

        match data.remove(MODEL_PICKER_KEY) {
            None | Some(Value::Null) => {}
            Some(picker) => {
                let picker: ModelPicker =
                    serde_json::from_value(picker).context(MODEL_PICKER_KEY)?;
                data.insert(MODEL_PICKER_KEY.to_owned(), serde_json::to_value(picker)?);
            }
        }
        Ok(Self { data })
    }

    pub fn data(&self) -> &Map<String, Value> {
        &self.data
    }

    fn enabled_plugins(&self) -> Option<&Map<String, Value>> {
        self.data.get(ENABLED_PLUGINS_KEY)?.as_object()
    }

    fn is_enabled(&self, plugin: &str) -> bool {
        self.enabled_plugins()
            .and_then(|plugins| plugins.get(plugin))
            .is_some_and(|enabled| *enabled == Value::Bool(true))
    }

    fn prune_enabled_plugins(&mut self, installed: &[String]) -> Vec<String> {
        let Some(plugins) = self
            .data
            .get_mut(ENABLED_PLUGINS_KEY)
            .and_then(Value::as_object_mut)
        else {
            return Vec::new();
        };
        let mut orphans: Vec<String> = plugins
            .keys()
            .filter(|plugin| !installed.contains(plugin))
            .cloned()
            .collect();
        orphans.sort();
        plugins.retain(|plugin, _| installed.contains(plugin));
        if plugins.is_empty() {
            self.data.remove(ENABLED_PLUGINS_KEY);
        }
        orphans
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct MarketplaceSource {
    pub source: String,
    repo: String,
    path: String,
    url: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct KnownMarketplace {
    pub source: MarketplaceSource,
    install_location: String,
}

impl KnownMarketplace {
    fn source_string(&self) -> Result<&str> {
        let source = &self.source;
        match source.source.as_str() {
            "github" => Ok(&source.repo),
            "directory" | "local" if source.path.is_empty() => Ok(&self.install_location),
            "directory" | "local" => Ok(&source.path),
            "git" => Ok(&source.url),
            other => bail!("unknown source type: {other}"),
        }
    }

    fn is_local(&self) -> bool {
        matches!(self.source.source.as_str(), "local" | "directory")
    }
}

/// The registered marketplaces by name, in the order the registry lists them.
pub struct KnownMarketplaces(Vec<(String, KnownMarketplace)>);

impl KnownMarketplaces {
    pub fn get(&self, name: &str) -> Option<&KnownMarketplace> {
        self.0
            .iter()
            .find_map(|(known, info)| (known == name).then_some(info))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(name, _)| name.as_str())
    }

    fn find_by_source(&self, source: &str) -> Result<Option<&str>> {
        let target = normalize_source(source)?;
        for (name, info) in &self.0 {
            if normalize_source(info.source_string()?)? == target {
                return Ok(Some(name));
            }
        }
        Ok(None)
    }

    pub fn find_by_path(&self, path: &Path) -> Result<Option<&str>> {
        let target = resolve(path)?;
        for (name, info) in &self.0 {
            for candidate in [&info.source.path, &info.install_location] {
                if !candidate.is_empty() && resolve(&expand_user(candidate)?)? == target {
                    return Ok(Some(name));
                }
            }
        }
        Ok(None)
    }
}

#[derive(Debug, PartialEq)]
pub struct MarketplaceState {
    pub source: String,
    pub source_type: String,
    pub enabled: BTreeSet<String>,
    pub disabled: BTreeSet<String>,
}

impl MarketplaceState {
    pub fn installed(&self) -> BTreeSet<String> {
        self.enabled.union(&self.disabled).cloned().collect()
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct InstalledPlugins {
    plugins: Map<String, Value>,
}

#[derive(Deserialize)]
struct ManifestPlugin {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Manifest {
    plugins: Vec<ManifestPlugin>,
}

pub struct ClaudeDir {
    pub dir: PathBuf,
    config_path: PathBuf,
}

impl ClaudeDir {
    /// `$CLAUDE_CONFIG_DIR` holding its own `.claude.json`, or `~/.claude` beside `~/.claude.json`.
    pub fn locate() -> Result<Self> {
        match env::var_os(CONFIG_DIR_ENV).filter(|dir| !dir.is_empty()) {
            Some(dir) => Ok(Self::at(PathBuf::from(dir))),
            None => {
                let home = dirs::home()?;
                Ok(Self {
                    dir: home.join(".claude"),
                    config_path: home.join(CONFIG_FILENAME),
                })
            }
        }
    }

    fn at(dir: PathBuf) -> Self {
        Self {
            config_path: dir.join(CONFIG_FILENAME),
            dir,
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    fn plugins_dir(&self) -> PathBuf {
        self.dir.join("plugins")
    }

    pub fn claude_md_path(&self) -> PathBuf {
        self.dir.join("CLAUDE.md")
    }

    pub fn keybindings_path(&self) -> PathBuf {
        self.dir.join("keybindings.json")
    }

    pub fn hidden_root(&self) -> PathBuf {
        self.dir.join(HIDDEN_MARKETPLACE_NAME)
    }

    pub fn hidden_marketplace_dir(&self) -> PathBuf {
        self.hidden_root().join("marketplace")
    }

    pub fn hidden_manifest_path(&self) -> PathBuf {
        manifest_path(&self.hidden_marketplace_dir())
    }

    pub fn load_config(&self) -> Result<ClaudeJson> {
        ClaudeJson::from_map(load_json(&self.config_path)?)
            .with_context(|| self.config_path.display().to_string())
    }

    pub fn save_config(&self, config: &ClaudeJson, log: Log) -> Result<()> {
        write_if_different(&self.config_path, config.dump()?.as_bytes(), log)
    }

    pub fn load_settings(&self) -> Result<Settings> {
        let path = self.settings_path();
        Settings::from_map(load_json(&path)?).with_context(|| path.display().to_string())
    }

    pub fn save_settings(&self, settings: &Settings, log: Log) -> Result<()> {
        let text = dump_json(Value::Object(settings.data.clone()))?;
        write_if_different(&self.settings_path(), text.as_bytes(), log)
    }

    /// The `<plugin>@<marketplace>` names of the installed plugins, in registry order.
    pub fn installed_plugins(&self) -> Result<Vec<String>> {
        let path = self.plugins_dir().join("installed_plugins.json");
        let installed: InstalledPlugins = serde_json::from_value(Value::Object(load_json(&path)?))
            .with_context(|| path.display().to_string())?;
        Ok(installed
            .plugins
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    pub fn known_marketplaces(&self) -> Result<KnownMarketplaces> {
        let path = self.plugins_dir().join("known_marketplaces.json");
        let mut known = Vec::new();
        for (name, info) in load_json(&path)? {
            let info = serde_json::from_value(info)
                .with_context(|| format!("{}: {name}", path.display()))?;
            known.push((name, info));
        }
        Ok(KnownMarketplaces(known))
    }

    pub fn current_state(&self) -> Result<Vec<(String, MarketplaceState)>> {
        let known = self.known_marketplaces()?;
        let settings = self.load_settings()?;
        let mut state = Vec::new();
        for (name, info) in &known.0 {
            state.push((
                name.clone(),
                MarketplaceState {
                    source: info.source_string()?.to_owned(),
                    source_type: info.source.source.clone(),
                    enabled: BTreeSet::new(),
                    disabled: BTreeSet::new(),
                },
            ));
        }
        for installed in self.installed_plugins()? {
            let Some(reference) = PluginRef::parse(&installed) else {
                continue;
            };
            let Some((_, marketplace)) = state
                .iter_mut()
                .find(|(name, _)| *name == reference.marketplace)
            else {
                continue;
            };
            if settings.is_enabled(&installed) {
                marketplace.enabled.insert(reference.plugin);
            } else {
                marketplace.disabled.insert(reference.plugin);
            }
        }
        Ok(state)
    }

    pub fn valid_plugins(&self, marketplace: &str) -> Result<BTreeSet<String>> {
        let known = self.known_marketplaces()?;
        let Some(info) = known
            .get(marketplace)
            .filter(|info| !info.install_location.is_empty())
        else {
            return Ok(BTreeSet::new());
        };
        let path = manifest_path(Path::new(&info.install_location));
        let manifest: Manifest = serde_json::from_value(Value::Object(load_json(&path)?))
            .with_context(|| path.display().to_string())?;
        Ok(manifest
            .plugins
            .into_iter()
            .map(|plugin| plugin.name)
            .collect())
    }

    pub fn is_local_marketplace(&self, marketplace: &str) -> Result<bool> {
        Ok(self
            .known_marketplaces()?
            .get(marketplace)
            .is_some_and(KnownMarketplace::is_local))
    }

    pub fn find_marketplace_by_source(&self, source: &str) -> Result<Option<String>> {
        let known = self.known_marketplaces()?;
        Ok(known.find_by_source(source)?.map(str::to_owned))
    }

    pub fn find_hidden_marketplace(&self) -> Result<Option<String>> {
        let known = self.known_marketplaces()?;
        let name = known.find_by_path(&self.hidden_marketplace_dir())?;
        Ok(name.map(str::to_owned))
    }

    pub fn clear_plugin_cache(&self, plugin: &PluginRef) -> Result<()> {
        let cache = self
            .plugins_dir()
            .join("cache")
            .join(&plugin.marketplace)
            .join(&plugin.plugin);
        match fs::remove_dir_all(&cache) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("removing {}", cache.display())),
        }
    }

    /// Drops `enabledPlugins` entries with no installed plugin. They linger when an install record
    /// vanishes outside this tool, and a stale entry for the same plugin name under another
    /// marketplace makes `claude plugin uninstall` fail.
    pub fn prune_enabled_plugins(&self, log: Log) -> Result<()> {
        let mut settings = self.load_settings()?;
        let orphans = settings.prune_enabled_plugins(&self.installed_plugins()?);
        for orphan in &orphans {
            log.warn(format_args!(
                "pruning orphaned enabledPlugins entry: {orphan}"
            ));
        }
        if !orphans.is_empty() {
            self.save_settings(&settings, log)?;
        }
        Ok(())
    }
}

fn manifest_path(marketplace_dir: &Path) -> PathBuf {
    marketplace_dir
        .join(".claude-plugin")
        .join("marketplace.json")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const LOG: Log = Log { quiet: true };

    /// The pinned `env` entry every saved `settings.json` carries.
    fn pinned_env() -> Value {
        json!({"CLAUDE_CODE_GB_DISK_CACHE_WHEN_TELEMETRY_OFF": "1"})
    }

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn claude_dir() -> (tempfile::TempDir, ClaudeDir) {
        let temp = tempfile::tempdir().unwrap();
        let dir = ClaudeDir::at(temp.path().to_owned());
        fs::create_dir(dir.plugins_dir()).unwrap();
        (temp, dir)
    }

    fn write_json(path: PathBuf, value: Value) {
        fs::write(path, value.to_string()).unwrap();
    }

    #[test]
    fn config_dump_pins_flags_sorts_keys_and_drops_shallow_nulls() {
        let config = ClaudeJson::from_map(object(json!({
            "z": 1,
            "gone": null,
            "a": {"kept": null},
            "cachedGrowthBookFeatures": {"other": 1},
            "projects": {"/p": {"gone": null, "deep": {"kept": null}}},
        })))
        .unwrap();
        assert_eq!(
            config.dump().unwrap(),
            r#"{
  "a": {
    "kept": null
  },
  "cachedGrowthBookFeatures": {
    "tengu_agents_md_mod": true
  },
  "projects": {
    "/p": {
      "deep": {
        "kept": null
      }
    }
  },
  "z": 1
}
"#
        );
    }

    #[test]
    fn config_of_a_missing_file_has_pins_and_no_projects() {
        let config = ClaudeJson::from_map(Map::new()).unwrap();
        assert_eq!(
            config.dump().unwrap(),
            "{\n  \"cachedGrowthBookFeatures\": {\n    \"tengu_agents_md_mod\": true\n  },\n  \"projects\": {}\n}\n"
        );
    }

    #[test]
    fn trust_marks_the_root_and_every_project() {
        let mut config =
            ClaudeJson::from_map(object(json!({"projects": {"/p": {"x": 1}}}))).unwrap();
        assert!(!config.trusts_everything());
        config.trust_everything();
        assert!(config.trusts_everything());
        assert_eq!(
            config.data[PROJECTS_KEY],
            json!({
                "/p": {"x": 1, "hasTrustDialogAccepted": true},
                "/": {"hasTrustDialogAccepted": true},
            })
        );
    }

    #[test]
    fn config_rejects_mistyped_keys() {
        assert!(ClaudeJson::from_map(object(json!({"theme": 1}))).is_err());
        assert!(ClaudeJson::from_map(object(json!({"projects": []}))).is_err());
        assert!(ClaudeJson::from_map(object(json!({"projects": {"/p": 1}}))).is_err());
        let trust = json!({"projects": {"/p": {"hasTrustDialogAccepted": "yes"}}});
        assert!(ClaudeJson::from_map(object(trust)).is_err());
    }

    #[test]
    fn mcp_servers_are_removed_when_empty() {
        let mut config = ClaudeJson::from_map(Map::new()).unwrap();
        config.set_mcp_servers(object(json!({"a": {"command": "x"}})));
        assert_eq!(
            Value::Object(config.mcp_servers().unwrap()),
            json!({"a": {"command": "x"}})
        );
        config.set_mcp_servers(Map::new());
        assert!(!config.data.contains_key(MCP_SERVERS_KEY));
    }

    #[test]
    fn settings_pin_env_and_drop_what_a_save_leaves_out() {
        let settings = Settings::from_map(object(json!({
            "enabledPlugins": {},
            "modelPicker": {"options": [{"model": "m", "label": null}]},
            "other": null,
        })))
        .unwrap();
        assert_eq!(
            Value::Object(settings.data),
            json!({
                "env": pinned_env(),
                "modelPicker": {"options": [{"model": "m"}]},
                "other": null,
            })
        );
        let empty = Settings::from_map(object(json!({"modelPicker": null}))).unwrap();
        assert_eq!(Value::Object(empty.data), json!({"env": pinned_env()}));
    }

    #[test]
    fn settings_reject_mistyped_keys() {
        assert!(Settings::from_map(object(json!({"env": []}))).is_err());
        assert!(Settings::from_map(object(json!({"enabledPlugins": {"a@b": "yes"}}))).is_err());
        let picker = json!({"modelPicker": {"options": [{"model": "m", "extra": 1}]}});
        assert!(Settings::from_map(object(picker)).is_err());
    }

    #[test]
    fn current_state_splits_installed_plugins_by_enabled() {
        let (_temp, dir) = claude_dir();
        write_json(
            dir.plugins_dir().join("known_marketplaces.json"),
            json!({
                "hub": {"source": {"source": "github", "repo": "o/r"}, "installLocation": "/x"},
                "dir": {"source": {"source": "directory"}, "installLocation": "/local"},
            }),
        );
        write_json(
            dir.plugins_dir().join("installed_plugins.json"),
            json!({"plugins": {"on@hub": [], "off@hub": [], "x@gone": [], "bare": []}}),
        );
        write_json(
            dir.settings_path(),
            json!({"enabledPlugins": {"on@hub": true, "off@hub": false}}),
        );

        let state = dir.current_state().unwrap();

        assert_eq!(state.len(), 2);
        let hub = &state.iter().find(|(name, _)| name == "hub").unwrap().1;
        assert_eq!(hub.source, "o/r");
        assert_eq!(hub.enabled, BTreeSet::from(["on".to_owned()]));
        assert_eq!(hub.disabled, BTreeSet::from(["off".to_owned()]));
        let local = &state.iter().find(|(name, _)| name == "dir").unwrap().1;
        assert_eq!(local.source, "/local");
        assert!(dir.is_local_marketplace("dir").unwrap());
        assert!(!dir.is_local_marketplace("hub").unwrap());
        assert_eq!(
            dir.find_marketplace_by_source("o/r").unwrap().as_deref(),
            Some("hub")
        );
    }

    #[test]
    fn unknown_marketplace_source_types_are_errors() {
        let (_temp, dir) = claude_dir();
        write_json(
            dir.plugins_dir().join("known_marketplaces.json"),
            json!({"odd": {"source": {"source": "npm"}}}),
        );
        let error = dir.current_state().unwrap_err();
        assert_eq!(error.to_string(), "unknown source type: npm");
    }

    #[test]
    fn prune_drops_enabled_plugins_that_are_not_installed() {
        let (_temp, dir) = claude_dir();
        write_json(
            dir.plugins_dir().join("installed_plugins.json"),
            json!({"plugins": {"kept@hub": []}}),
        );
        write_json(
            dir.settings_path(),
            json!({"enabledPlugins": {"kept@hub": true, "orphan@hub": true}}),
        );

        dir.prune_enabled_plugins(LOG).unwrap();

        let saved: Value = serde_json::from_slice(&fs::read(dir.settings_path()).unwrap()).unwrap();
        assert_eq!(
            saved,
            json!({"enabledPlugins": {"kept@hub": true}, "env": pinned_env()})
        );
    }

    #[test]
    fn hidden_marketplace_is_found_by_its_path() {
        let (_temp, dir) = claude_dir();
        let hidden = dir.hidden_marketplace_dir().display().to_string();
        write_json(
            dir.plugins_dir().join("known_marketplaces.json"),
            json!({"renamed": {"source": {"source": "directory", "path": hidden}}}),
        );
        assert_eq!(
            dir.find_hidden_marketplace().unwrap().as_deref(),
            Some("renamed")
        );
    }

    #[test]
    fn valid_plugins_come_from_the_cached_manifest() {
        let (temp, dir) = claude_dir();
        let location = temp.path().join("market");
        fs::create_dir_all(location.join(".claude-plugin")).unwrap();
        write_json(
            manifest_path(&location),
            json!({"name": "m", "plugins": [{"name": "a", "source": "./a"}, {"name": "b"}]}),
        );
        write_json(
            dir.plugins_dir().join("known_marketplaces.json"),
            json!({"m": {"source": {"source": "git", "url": "u"}, "installLocation": location}}),
        );
        assert_eq!(
            dir.valid_plugins("m").unwrap(),
            BTreeSet::from(["a".to_owned(), "b".to_owned()])
        );
        assert!(dir.valid_plugins("unknown").unwrap().is_empty());
    }
}
