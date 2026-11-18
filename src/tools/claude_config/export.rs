//! `export`: write the spec of Claude's current installed state to a directory.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::tools::claude_config::files::{
    Log, mirror_tree, mirror_yaml_dir, read_optional, read_text, sync_file, unified_diff,
    write_if_different, yaml_dir_diff,
};
use crate::tools::claude_config::paths::collapse_home;
use crate::tools::claude_config::spec::{
    ARTIFACT_DIRS, CLAUDE_MD_FILENAME, HiddenManifest, KEYBINDINGS_FILENAME, MARKETPLACES_DIRNAME,
    MCP_DIRNAME, MarketplaceEntry, MarketplaceType, McpServer, PLUGINS_DIRNAME, PluginSpec,
    SPEC_FILENAME, Spec, load_mcp_specs,
};
use crate::tools::claude_config::state::{ClaudeDir, UNMANAGED_SETTINGS};
use crate::yaml;

struct Current {
    spec: Spec,
    marketplaces: Vec<MarketplaceEntry>,
    plugins: BTreeMap<String, PluginSpec>,
}

impl Current {
    fn read(claude: &ClaudeDir) -> Result<Self> {
        let config = claude.load_config()?.config_spec()?;
        let settings: Map<String, Value> = claude
            .load_settings()?
            .data()
            .iter()
            .filter(|(key, _)| !UNMANAGED_SETTINGS.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let spec = Spec {
            config: Some(config),
            settings: (!settings.is_empty()).then_some(settings),
        };

        let hidden = claude.find_hidden_marketplace()?;
        let mut marketplaces = Vec::new();
        let mut plugins = BTreeMap::new();
        for (name, state) in claude.current_state()? {
            if Some(&name) == hidden.as_ref() {
                let manifest_path = claude.hidden_manifest_path();
                let manifest = read_optional(&manifest_path)?.unwrap_or_else(|| b"{}".to_vec());
                let manifest = serde_json::from_slice(&manifest)
                    .map_err(anyhow::Error::from)
                    .and_then(HiddenManifest::from_value)
                    .with_context(|| manifest_path.display().to_string())?;
                let mut sources = manifest.into_sources();
                let installed = state.installed();
                let missing: Vec<&str> = installed
                    .iter()
                    .filter(|plugin| !sources.contains_key(*plugin))
                    .map(String::as_str)
                    .collect();
                if !missing.is_empty() {
                    bail!(
                        "plugins installed from {name} but missing from {}: {}",
                        manifest_path.display(),
                        missing.join(", ")
                    );
                }
                for plugin in installed {
                    if let Some(source) = sources.remove(&plugin) {
                        let enabled = state.enabled.contains(&plugin);
                        plugins.insert(plugin, source.into_plugin(enabled));
                    }
                }
                continue;
            }

            let kind = match state.source_type.as_str() {
                "" => MarketplaceType::Git,
                kind => MarketplaceType::parse(kind)?,
            };
            let source = match kind {
                MarketplaceType::Directory => collapse_home(&state.source)?,
                _ => state.source.clone(),
            };
            let available = claude.valid_plugins(&name)?;
            marketplaces.push(MarketplaceEntry::new(
                Some(name),
                kind,
                source,
                state.enabled,
                state.disabled,
                available,
            )?);
        }
        Ok(Self {
            spec,
            marketplaces,
            plugins,
        })
    }
}

/// An existing spec file in the form a fresh export has, for diffing only. A file that predates
/// the current schema is not an error here: it is shown as it parses.
fn diff_baseline(text: &str) -> Result<String> {
    let data = yaml::load(text).context("invalid YAML")?;
    match Spec::from_value(data.clone()) {
        Ok(spec) => spec.to_yaml(),
        Err(_) => yaml::dump_sorted(&data),
    }
}

fn dump_all<T: Serialize>(documents: &BTreeMap<String, T>) -> Result<BTreeMap<String, String>> {
    documents
        .iter()
        .map(|(name, document)| Ok((name.clone(), yaml::dump(document)?)))
        .collect()
}

fn lossy(data: Option<&[u8]>) -> String {
    String::from_utf8_lossy(data.unwrap_or_default()).into_owned()
}

pub fn run(
    directory: &Path,
    force: bool,
    dry_run: bool,
    claude: &ClaudeDir,
    log: Log,
) -> Result<ExitCode> {
    if directory.exists() && !directory.is_dir() {
        bail!("not a directory: {}", directory.display());
    }
    let yaml_path = directory.join(SPEC_FILENAME);
    let md_path = directory.join(CLAUDE_MD_FILENAME);
    let keybindings_path = directory.join(KEYBINDINGS_FILENAME);
    let mcp_dir = directory.join(MCP_DIRNAME);
    let marketplaces_dir = directory.join(MARKETPLACES_DIRNAME);
    let plugins_dir = directory.join(PLUGINS_DIRNAME);

    let current = Current::read(claude)?;
    let desired_yaml = current.spec.to_yaml()?;
    let desired_md = read_optional(&claude.claude_md_path())?;
    let desired_keybindings = read_optional(&claude.keybindings_path())?;

    let mut mcp_servers = BTreeMap::new();
    for (name, server) in claude.load_config()?.mcp_servers()? {
        let server = McpServer::from_value(server)
            .with_context(|| format!("mcp server {name}"))?
            .into_config();
        mcp_servers.insert(name, server);
    }
    // A disabled server is in the spec only, never in the config; carried over so the export does
    // not prune it.
    for (name, server) in load_mcp_specs(directory)? {
        if !server.enabled() {
            mcp_servers.entry(name).or_insert(server);
        }
    }
    let marketplaces: BTreeMap<String, &MarketplaceEntry> = current
        .marketplaces
        .iter()
        .filter_map(|entry| Some((entry.name.clone()?, entry)))
        .collect();

    let mcp_servers = dump_all(&mcp_servers)?;
    let marketplaces = dump_all(&marketplaces)?;
    let plugins = dump_all(&current.plugins)?;

    if !log.quiet {
        let baseline_yaml = match read_optional(&yaml_path)? {
            Some(_) => diff_baseline(&read_text(&yaml_path)?)
                .with_context(|| yaml_path.display().to_string())?,
            None => String::new(),
        };
        let baseline_md = lossy(read_optional(&md_path)?.as_deref());
        print!(
            "{}{}{}{}{}",
            unified_diff(SPEC_FILENAME, &baseline_yaml, &desired_yaml),
            unified_diff(
                CLAUDE_MD_FILENAME,
                &baseline_md,
                &lossy(desired_md.as_deref())
            ),
            yaml_dir_diff(&mcp_dir, MCP_DIRNAME, &mcp_servers)?,
            yaml_dir_diff(&marketplaces_dir, MARKETPLACES_DIRNAME, &marketplaces)?,
            yaml_dir_diff(&plugins_dir, PLUGINS_DIRNAME, &plugins)?,
        );
    }

    if !dry_run {
        let existing = [
            &yaml_path,
            &md_path,
            &keybindings_path,
            &mcp_dir,
            &marketplaces_dir,
            &plugins_dir,
        ]
        .into_iter()
        .cloned()
        .chain(ARTIFACT_DIRS.iter().map(|name| directory.join(name)))
        .find(|path| path.exists());
        if let Some(existing) = existing
            && !force
        {
            bail!("path exists (use -f to overwrite): {}", existing.display());
        }
        fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        write_if_different(&yaml_path, desired_yaml.as_bytes(), log)?;
        sync_file(&md_path, desired_md.as_deref(), log)?;
        sync_file(&keybindings_path, desired_keybindings.as_deref(), log)?;
    }

    for name in ARTIFACT_DIRS {
        mirror_tree(&claude.dir.join(name), &directory.join(name), dry_run, log)?;
    }
    mirror_yaml_dir(&mcp_dir, &mcp_servers, dry_run, log)?;
    mirror_yaml_dir(&marketplaces_dir, &marketplaces, dry_run, log)?;
    mirror_yaml_dir(&plugins_dir, &plugins, dry_run, log)?;
    Ok(ExitCode::SUCCESS)
}
