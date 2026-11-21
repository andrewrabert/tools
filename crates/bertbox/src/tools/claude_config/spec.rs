//! The spec directory: `claude-config.yaml` and its `mcp/`, `marketplaces/` and `plugins/` files.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::tools::claude_config::files::{read_text, yaml_files};
use crate::yaml;

pub const SPEC_FILENAME: &str = "claude-config.yaml";
pub const CLAUDE_MD_FILENAME: &str = "CLAUDE.md";
pub const KEYBINDINGS_FILENAME: &str = "keybindings.json";
pub const MCP_DIRNAME: &str = "mcp";
pub const MARKETPLACES_DIRNAME: &str = "marketplaces";
pub const PLUGINS_DIRNAME: &str = "plugins";

/// User-authored directories mirrored verbatim between the spec directory and the config directory.
pub const ARTIFACT_DIRS: [&str; 7] = [
    "agents",
    "commands",
    "skills",
    "output-styles",
    "rules",
    "workflows",
    "themes",
];

pub const HIDDEN_MARKETPLACE_NAME: &str = "claude-config";
const HIDDEN_MARKETPLACE_DESCRIPTION: &str =
    "managed by claude-config (github.com/andrewrabert) - do not edit";

/// The spec key that trusts every project; it expands to `projects` entries, not a `.claude.json` key.
const TRUST_KEY: &str = "hasTrustDialogAccepted";

fn enabled() -> bool {
    true
}

fn is_enabled(enabled: &bool) -> bool {
    *enabled
}

fn from_value<T: DeserializeOwned>(value: Value) -> Result<T> {
    Ok(serde_json::from_value(value)?)
}

#[derive(Deserialize, Serialize, Default, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Map<String, Value>>,
}

impl Spec {
    pub fn from_value(value: Value) -> Result<Self> {
        from_value(value)
    }

    pub fn parse(text: &str) -> Result<Self> {
        Self::from_value(yaml::load(text).context("invalid YAML")?)
    }

    pub fn read(path: &Path) -> Result<Self> {
        let path = if path.is_dir() {
            path.join(SPEC_FILENAME)
        } else {
            path.to_owned()
        };
        if !path.exists() {
            bail!("spec file not found: {}", path.display());
        }
        Self::parse(&read_text(&path)?).with_context(|| path.display().to_string())
    }

    pub fn to_yaml(&self) -> Result<String> {
        yaml::dump_sorted(&serde_json::to_value(self)?)
    }
}

#[derive(Deserialize, Serialize, Default, Debug, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConfigSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_connect_ide: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_memory_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass_permissions_mode_accepted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsp_recommendation_disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_trust_dialog_accepted: Option<bool>,
}

impl ConfigSpec {
    const KEYS: [&str; 7] = [
        "autoConnectIde",
        "autoMemoryEnabled",
        "bypassPermissionsModeAccepted",
        "editorMode",
        "lspRecommendationDisabled",
        "theme",
        "verbose",
    ];

    pub fn from_claude_json(data: &Map<String, Value>) -> Result<Self> {
        let keys: Map<String, Value> = Self::KEYS
            .iter()
            .filter_map(|key| Some(((*key).to_owned(), data.get(*key)?.clone())))
            .collect();
        from_value(Value::Object(keys))
    }

    pub fn claude_json_keys(&self) -> Result<Map<String, Value>> {
        let Value::Object(mut keys) = serde_json::to_value(self)? else {
            bail!("config spec did not serialize to a mapping");
        };
        keys.remove(TRUST_KEY);
        Ok(keys)
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MarketplaceType {
    Github,
    Git,
    Directory,
    Local,
    Url,
    Npm,
    GitSubdir,
}

impl MarketplaceType {
    pub fn parse(name: &str) -> Result<Self> {
        from_value(Value::String(name.to_owned()))
            .with_context(|| format!("marketplace type {name:?}"))
    }
}

fn serialize_plugins<S: Serializer>(
    plugins: &BTreeSet<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut names: Vec<&String> = plugins.iter().collect();
    names.sort_by_key(|name| name.to_lowercase());
    serializer.collect_seq(names)
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MarketplaceEntry {
    #[serde(skip)]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: MarketplaceType,
    pub source: String,
    #[serde(default, serialize_with = "serialize_plugins")]
    pub enabled: BTreeSet<String>,
    #[serde(default, serialize_with = "serialize_plugins")]
    pub disabled: BTreeSet<String>,
    #[serde(default, serialize_with = "serialize_plugins")]
    pub available: BTreeSet<String>,
}

impl MarketplaceEntry {
    pub fn new(
        name: Option<String>,
        kind: MarketplaceType,
        source: String,
        enabled: BTreeSet<String>,
        disabled: BTreeSet<String>,
        available: BTreeSet<String>,
    ) -> Result<Self> {
        let overlap: Vec<&str> = enabled
            .intersection(&disabled)
            .map(String::as_str)
            .collect();
        if !overlap.is_empty() {
            bail!(
                "plugins in both enabled and disabled: {}",
                overlap.join(", ")
            );
        }
        let available = available
            .into_iter()
            .filter(|plugin| !enabled.contains(plugin) && !disabled.contains(plugin))
            .collect();
        Ok(Self {
            name,
            kind,
            source,
            enabled,
            disabled,
            available,
        })
    }

    fn from_file(name: &str, mut data: Value) -> Result<Self> {
        // The name is the file's; a `name` key in the file is not read.
        if let Some(data) = data.as_object_mut() {
            data.remove("name");
        }
        let entry: Self = from_value(data)?;
        Self::new(
            Some(name.to_owned()),
            entry.kind,
            entry.source,
            entry.enabled,
            entry.disabled,
            entry.available,
        )
    }

    pub fn installed(&self) -> BTreeSet<String> {
        self.enabled.union(&self.disabled).cloned().collect()
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Default, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StdioTransport {
    #[default]
    Stdio,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteTransport {
    Http,
    Sse,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct McpStdioServer {
    #[serde(default, rename = "type")]
    pub transport: StdioTransport,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpRemoteServer {
    #[serde(rename = "type")]
    pub transport: RemoteTransport,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers_helper: Option<String>,
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
}

/// An `mcp/<name>.yaml` file, or a `.claude.json` `mcpServers` entry. `enabled` exists only in the
/// spec, and is written only when false.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum McpServer {
    Stdio(McpStdioServer),
    Remote(McpRemoteServer),
}

impl McpServer {
    /// Reads a server mapping. Without a `type`, a `url` means http and anything else stdio.
    pub fn from_value(mut data: Value) -> Result<Self> {
        let Some(server) = data.as_object_mut() else {
            bail!("expected a mapping");
        };
        let transport = match server.get("type") {
            Some(Value::String(transport)) => transport.clone(),
            Some(other) => bail!("invalid type: {other}"),
            None if server.contains_key("url") => {
                server.insert("type".to_owned(), Value::String("http".to_owned()));
                "http".to_owned()
            }
            None => "stdio".to_owned(),
        };
        match transport.as_str() {
            "stdio" => Ok(McpServer::Stdio(from_value(data)?)),
            "http" | "sse" => Ok(McpServer::Remote(from_value(data)?)),
            other => bail!("unknown type {other:?}, expected stdio, http or sse"),
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            McpServer::Stdio(server) => server.enabled,
            McpServer::Remote(server) => server.enabled,
        }
    }

    /// The server as `.claude.json` holds it, which never carries `enabled`.
    pub fn into_config(mut self) -> Self {
        match &mut self {
            McpServer::Stdio(server) => server.enabled = true,
            McpServer::Remote(server) => server.enabled = true,
        }
        self
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PluginSpec {
    Github {
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
        #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
        enabled: bool,
    },
    Git {
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
        #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
        enabled: bool,
    },
    GitSubdir {
        source: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
        #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
        enabled: bool,
    },
    Npm {
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
        #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
        enabled: bool,
    },
}

impl PluginSpec {
    pub fn enabled(&self) -> bool {
        match self {
            PluginSpec::Github { enabled, .. }
            | PluginSpec::Git { enabled, .. }
            | PluginSpec::GitSubdir { enabled, .. }
            | PluginSpec::Npm { enabled, .. } => *enabled,
        }
    }

    fn to_source(&self) -> PluginSource {
        match self.clone() {
            PluginSpec::Github {
                source, r#ref, sha, ..
            } => PluginSource::Github {
                repo: source,
                r#ref,
                sha,
            },
            PluginSpec::Git {
                source, r#ref, sha, ..
            } => PluginSource::Git {
                url: source,
                r#ref,
                sha,
            },
            PluginSpec::GitSubdir {
                source,
                path,
                r#ref,
                sha,
                ..
            } => PluginSource::GitSubdir {
                url: source,
                path,
                r#ref,
                sha,
            },
            PluginSpec::Npm {
                source,
                version,
                registry,
                ..
            } => PluginSource::Npm {
                package: source,
                version,
                registry,
            },
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PluginSource {
    Github {
        repo: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
    },
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
    },
    GitSubdir {
        url: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha: Option<String>,
    },
    Npm {
        package: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
    },
}

impl PluginSource {
    pub fn into_plugin(self, enabled: bool) -> PluginSpec {
        match self {
            PluginSource::Github { repo, r#ref, sha } => PluginSpec::Github {
                source: repo,
                r#ref,
                sha,
                enabled,
            },
            PluginSource::Git { url, r#ref, sha } => PluginSpec::Git {
                source: url,
                r#ref,
                sha,
                enabled,
            },
            PluginSource::GitSubdir {
                url,
                path,
                r#ref,
                sha,
            } => PluginSpec::GitSubdir {
                source: url,
                path,
                r#ref,
                sha,
                enabled,
            },
            PluginSource::Npm {
                package,
                version,
                registry,
            } => PluginSpec::Npm {
                source: package,
                version,
                registry,
                enabled,
            },
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct HiddenManifestPlugin {
    name: String,
    source: PluginSource,
}

/// The generated marketplace manifest that wraps the plugin specs, so Claude can install them.
#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HiddenManifest {
    #[serde(default = "HiddenManifest::default_name")]
    name: String,
    #[serde(default = "HiddenManifest::default_owner")]
    owner: BTreeMap<String, String>,
    #[serde(default = "HiddenManifest::default_metadata")]
    metadata: BTreeMap<String, String>,
    #[serde(default)]
    plugins: Vec<HiddenManifestPlugin>,
}

impl HiddenManifest {
    fn default_name() -> String {
        HIDDEN_MARKETPLACE_NAME.to_owned()
    }

    fn default_owner() -> BTreeMap<String, String> {
        BTreeMap::from([("name".to_owned(), HIDDEN_MARKETPLACE_NAME.to_owned())])
    }

    fn default_metadata() -> BTreeMap<String, String> {
        BTreeMap::from([(
            "description".to_owned(),
            HIDDEN_MARKETPLACE_DESCRIPTION.to_owned(),
        )])
    }

    pub fn from_specs(specs: &BTreeMap<String, PluginSpec>) -> Self {
        Self {
            name: Self::default_name(),
            owner: Self::default_owner(),
            metadata: Self::default_metadata(),
            plugins: specs
                .iter()
                .map(|(name, spec)| HiddenManifestPlugin {
                    name: name.clone(),
                    source: spec.to_source(),
                })
                .collect(),
        }
    }

    pub fn from_value(value: Value) -> Result<Self> {
        from_value(value)
    }

    pub fn into_sources(self) -> BTreeMap<String, PluginSource> {
        self.plugins
            .into_iter()
            .map(|plugin| (plugin.name, plugin.source))
            .collect()
    }
}

/// The parsed `<name>.yaml` files of a spec subdirectory, by name.
fn load_yaml_dir(dir: &Path) -> Result<Vec<(String, PathBuf, Value)>> {
    let mut documents = Vec::new();
    for (name, path) in yaml_files(dir)? {
        let value = yaml::load(&read_text(&path)?)
            .with_context(|| format!("invalid YAML in {}", path.display()))?;
        documents.push((name, path, value));
    }
    Ok(documents)
}

pub fn load_mcp_specs(directory: &Path) -> Result<BTreeMap<String, McpServer>> {
    let mut servers = BTreeMap::new();
    for (name, path, value) in load_yaml_dir(&directory.join(MCP_DIRNAME))? {
        let server = McpServer::from_value(value).with_context(|| path.display().to_string())?;
        servers.insert(name, server);
    }
    Ok(servers)
}

pub fn load_marketplaces(directory: &Path) -> Result<Vec<MarketplaceEntry>> {
    let mut entries = Vec::new();
    for (name, path, value) in load_yaml_dir(&directory.join(MARKETPLACES_DIRNAME))? {
        let entry = MarketplaceEntry::from_file(&name, value)
            .with_context(|| path.display().to_string())?;
        entries.push(entry);
    }
    Ok(entries)
}

pub fn load_plugins(directory: &Path) -> Result<BTreeMap<String, PluginSpec>> {
    let mut specs = BTreeMap::new();
    for (name, path, value) in load_yaml_dir(&directory.join(PLUGINS_DIRNAME))? {
        let spec: PluginSpec = from_value(value).with_context(|| path.display().to_string())?;
        specs.insert(name, spec);
    }
    Ok(specs)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginRef {
    pub plugin: String,
    pub marketplace: String,
}

impl PluginRef {
    pub fn new(plugin: &str, marketplace: &str) -> Self {
        Self {
            plugin: plugin.to_owned(),
            marketplace: marketplace.to_owned(),
        }
    }

    pub fn parse(reference: &str) -> Option<Self> {
        let (plugin, marketplace) = reference.rsplit_once('@')?;
        Some(Self::new(plugin, marketplace))
    }
}

impl fmt::Display for PluginRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.plugin, self.marketplace)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn spec_rejects_unknown_keys() {
        assert!(Spec::parse("config:\n  theme: dark\nsettings:\n  a: 1\n").is_ok());
        assert!(Spec::parse("marketplaces: []\n").is_err());
        assert!(Spec::parse("config:\n  nope: 1\n").is_err());
        assert!(Spec::parse("config:\n  verbose: loud\n").is_err());
        assert_eq!(Spec::parse("").unwrap(), Spec::default());
    }

    #[test]
    fn spec_yaml_sorts_keys_and_omits_unset() {
        let spec = Spec::parse(
            "settings:\n  z: 1\n  a:\n    - x\nconfig:\n  verbose: true\n  theme: dark\n  hasTrustDialogAccepted: false\n",
        )
        .unwrap();
        assert_eq!(
            spec.to_yaml().unwrap(),
            "config:\n  hasTrustDialogAccepted: false\n  theme: dark\n  verbose: true\nsettings:\n  a:\n  - x\n  z: 1\n"
        );
        assert_eq!(Spec::default().to_yaml().unwrap(), "{}\n");
    }

    #[test]
    fn config_keys_exclude_trust() {
        let spec = Spec::parse("config:\n  theme: dark\n  hasTrustDialogAccepted: true\n").unwrap();
        let keys = spec.config.unwrap().claude_json_keys().unwrap();
        assert_eq!(Value::Object(keys), json!({"theme": "dark"}));
    }

    #[test]
    fn config_from_claude_json_reads_only_its_keys() {
        let data = json!({"theme": "dark", "verbose": null, "numStartups": 4});
        let config = ConfigSpec::from_claude_json(data.as_object().unwrap()).unwrap();
        assert_eq!(config.theme.as_deref(), Some("dark"));
        assert_eq!(config.verbose, None);
        let invalid = json!({"theme": 4});
        assert!(ConfigSpec::from_claude_json(invalid.as_object().unwrap()).is_err());
    }

    #[test]
    fn marketplace_file_round_trips_in_key_order() {
        let data = yaml::load(
            "name: ignored\ntype: github\nsource: o/r\nenabled: [b, A]\navailable: [A, c]\n",
        )
        .unwrap();
        let entry = MarketplaceEntry::from_file("m", data).unwrap();
        assert_eq!(entry.name.as_deref(), Some("m"));
        assert_eq!(entry.available, set(&["c"]));
        assert_eq!(
            yaml::dump(&entry).unwrap(),
            "type: github\nsource: o/r\nenabled:\n- A\n- b\ndisabled: []\navailable:\n- c\n"
        );
    }

    #[test]
    fn marketplace_rejects_overlap_and_unknown_keys() {
        let overlap = yaml::load("type: git\nsource: u\nenabled: [a]\ndisabled: [a]\n").unwrap();
        let error = MarketplaceEntry::from_file("m", overlap).unwrap_err();
        assert_eq!(error.to_string(), "plugins in both enabled and disabled: a");
        let unknown = yaml::load("type: git\nsource: u\nextra: 1\n").unwrap();
        assert!(MarketplaceEntry::from_file("m", unknown).is_err());
        let kind = yaml::load("type: svn\nsource: u\n").unwrap();
        assert!(MarketplaceEntry::from_file("m", kind).is_err());
    }

    #[test]
    fn mcp_transport_is_inferred() {
        let stdio = McpServer::from_value(json!({"command": "x"})).unwrap();
        assert_eq!(
            yaml::dump(&stdio).unwrap(),
            "type: stdio\ncommand: x\nargs: []\nenv: {}\n"
        );
        let remote = McpServer::from_value(
            json!({"url": "https://x", "headersHelper": "h", "headers": {"b": "1", "a": "2"}}),
        )
        .unwrap();
        assert_eq!(
            yaml::dump(&remote).unwrap(),
            "type: http\nurl: https://x\nheaders:\n  a: \"2\"\n  b: \"1\"\nheadersHelper: h\n"
        );
    }

    #[test]
    fn mcp_enabled_is_written_only_when_false_and_never_to_config() {
        let server =
            McpServer::from_value(json!({"type": "sse", "url": "u", "enabled": false})).unwrap();
        assert!(!server.enabled());
        assert_eq!(
            yaml::dump(&server).unwrap(),
            "type: sse\nurl: u\nheaders: {}\nenabled: false\n"
        );
        assert_eq!(
            serde_json::to_value(server.into_config()).unwrap(),
            json!({"type": "sse", "url": "u", "headers": {}})
        );
    }

    #[test]
    fn mcp_rejects_unknown_keys_and_types() {
        assert!(McpServer::from_value(json!({"command": "x", "url": "u"})).is_err());
        assert!(McpServer::from_value(json!({"type": "ws", "url": "u"})).is_err());
        assert!(McpServer::from_value(json!({"type": "stdio"})).is_err());
        assert!(McpServer::from_value(json!({"command": "x", "env": {"PORT": 1}})).is_err());
        assert!(McpServer::from_value(json!(["x"])).is_err());
    }

    #[test]
    fn plugin_spec_round_trips_in_key_order() {
        let spec: PluginSpec = from_value(
            yaml::load(
                "sha: abc\npath: sub\nsource: https://x.git\ntype: git-subdir\nenabled: false\n",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            yaml::dump(&spec).unwrap(),
            "type: git-subdir\nsource: https://x.git\npath: sub\nsha: abc\nenabled: false\n"
        );
        let enabled: PluginSpec = from_value(json!({"type": "npm", "source": "pkg"})).unwrap();
        assert!(enabled.enabled());
        assert_eq!(yaml::dump(&enabled).unwrap(), "type: npm\nsource: pkg\n");
    }

    #[test]
    fn plugin_spec_rejects_keys_of_other_types() {
        assert!(from_value::<PluginSpec>(json!({"type": "github", "source": "o/r"})).is_ok());
        assert!(
            from_value::<PluginSpec>(json!({"type": "github", "source": "o/r", "path": "x"}))
                .is_err()
        );
        assert!(from_value::<PluginSpec>(json!({"source": "o/r"})).is_err());
        assert!(from_value::<PluginSpec>(json!({"type": "git-subdir", "source": "u"})).is_err());
    }

    #[test]
    fn hidden_manifest_wraps_specs_and_reads_back() {
        let specs = BTreeMap::from([
            (
                "b".to_owned(),
                PluginSpec::Npm {
                    source: "pkg".to_owned(),
                    version: Some("1".to_owned()),
                    registry: None,
                    enabled: false,
                },
            ),
            (
                "a".to_owned(),
                PluginSpec::Github {
                    source: "o/r".to_owned(),
                    r#ref: None,
                    sha: None,
                    enabled: true,
                },
            ),
        ]);
        let manifest = HiddenManifest::from_specs(&specs);
        let value = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            value,
            json!({
                "name": "claude-config",
                "owner": {"name": "claude-config"},
                "metadata": {"description": HIDDEN_MARKETPLACE_DESCRIPTION},
                "plugins": [
                    {"name": "a", "source": {"source": "github", "repo": "o/r"}},
                    {"name": "b", "source": {"source": "npm", "package": "pkg", "version": "1"}},
                ],
            })
        );
        let sources = HiddenManifest::from_value(value).unwrap().into_sources();
        assert_eq!(sources["b"].clone().into_plugin(false), specs["b"]);
        assert_eq!(sources["a"].clone().into_plugin(true), specs["a"]);
    }

    #[test]
    fn plugin_ref_splits_at_the_last_at() {
        let reference = PluginRef::parse("@scope/plugin@market").unwrap();
        assert_eq!(reference, PluginRef::new("@scope/plugin", "market"));
        assert_eq!(reference.to_string(), "@scope/plugin@market");
        assert_eq!(PluginRef::parse("plugin"), None);
    }
}
