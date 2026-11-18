//! `apply`: make Claude's installed state match a spec directory.
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::tools::claude_config::files::{
    Log, mirror_tree, read_optional, sync_file, write_if_different,
};
use crate::tools::claude_config::plugins::{Changes, ClaudeCli, Failures, is_local_source};
use crate::tools::claude_config::spec::{
    ARTIFACT_DIRS, CLAUDE_MD_FILENAME, HIDDEN_MARKETPLACE_NAME, HiddenManifest,
    KEYBINDINGS_FILENAME, MARKETPLACES_DIRNAME, MarketplaceEntry, MarketplaceType, PLUGINS_DIRNAME,
    PluginRef, Spec, load_marketplaces, load_mcp_specs, load_plugins,
};
use crate::tools::claude_config::state::{ClaudeDir, Settings, UNMANAGED_SETTINGS, dump_json};

/// Makes `settings.json` hold the spec's settings and nothing else, apart from the keys Claude
/// manages itself. Every key removed is named first, so runtime state Claude writes that this tool
/// does not know about never vanishes silently.
fn apply_settings(spec: &Map<String, Value>, claude: &ClaudeDir, log: Log) -> Result<()> {
    let current = claude.load_settings()?;
    let mut updated = Map::new();
    let mut removed = Vec::new();
    for (key, value) in current.data() {
        if UNMANAGED_SETTINGS.contains(&key.as_str()) {
            updated.insert(key.clone(), value.clone());
        } else if !spec.contains_key(key) {
            removed.push(key);
        }
    }
    removed.sort();
    for key in removed {
        log.warn(format_args!("removing settings key not in spec: {key}"));
    }
    updated.extend(spec.clone());
    if updated != *current.data() {
        claude.save_settings(&Settings::from_map(updated)?, log)?;
    }
    Ok(())
}

/// Sets the spec's config keys and enabled MCP servers in `.claude.json`. Everything is trusted,
/// always: the root and every project entry are marked trusted whatever the spec says, and trust
/// is never removed.
fn apply_config(spec: &Spec, directory: &Path, claude: &ClaudeDir, log: Log) -> Result<()> {
    let mut config = claude.load_config()?;
    if let Some(keys) = &spec.config {
        config.set_keys(keys.claude_json_keys()?);
    }
    config.trust_everything();
    let mut servers = Map::new();
    for (name, server) in load_mcp_specs(directory)? {
        if server.enabled() {
            servers.insert(name, serde_json::to_value(server.into_config())?);
        }
    }
    config.set_mcp_servers(servers);
    claude.save_config(&config, log)
}

pub fn run(directory: &Path, claude: &ClaudeDir, log: Log) -> Result<ExitCode> {
    if !directory.is_dir() {
        bail!("not a directory: {}", directory.display());
    }
    let cli = ClaudeCli::new(claude);
    let spec = Spec::read(directory)?;
    let mut marketplaces = load_marketplaces(directory)?;
    if marketplaces
        .iter()
        .any(|entry| entry.name.as_deref() == Some(HIDDEN_MARKETPLACE_NAME))
    {
        bail!(
            "{MARKETPLACES_DIRNAME}/{HIDDEN_MARKETPLACE_NAME}.yaml is reserved; declare plugins \
             in {PLUGINS_DIRNAME}/ instead"
        );
    }
    let plugin_specs = load_plugins(directory)?;

    let hidden_dir = claude.hidden_marketplace_dir();
    let known = claude.known_marketplaces()?;
    if known.contains(HIDDEN_MARKETPLACE_NAME)
        && known.find_by_path(&hidden_dir)? != Some(HIDDEN_MARKETPLACE_NAME)
    {
        bail!(
            "marketplace '{HIDDEN_MARKETPLACE_NAME}' is registered but does not point at {}; \
             refusing to adopt or remove it",
            hidden_dir.display()
        );
    }

    if !plugin_specs.is_empty() {
        let manifest = serde_json::to_value(HiddenManifest::from_specs(&plugin_specs))?;
        let manifest_path = claude.hidden_manifest_path();
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        write_if_different(&manifest_path, dump_json(manifest)?.as_bytes(), log)?;
        let (enabled, disabled): (BTreeSet<String>, BTreeSet<String>) = plugin_specs
            .iter()
            .map(|(name, spec)| (name.clone(), spec.enabled()))
            .fold(Default::default(), |mut sets, (name, enabled)| {
                if enabled {
                    sets.0.insert(name);
                } else {
                    sets.1.insert(name);
                }
                sets
            });
        marketplaces.push(MarketplaceEntry::new(
            None,
            MarketplaceType::Directory,
            hidden_dir.display().to_string(),
            enabled,
            disabled,
            BTreeSet::new(),
        )?);
    }

    claude.prune_enabled_plugins(log)?;
    let changes = Changes::compute(&claude.current_state()?, &marketplaces)?;
    let mut failures = Failures::default();

    let claude_md = read_optional(&directory.join(CLAUDE_MD_FILENAME))?;
    sync_file(&claude.claude_md_path(), claude_md.as_deref(), log)?;
    for name in ARTIFACT_DIRS {
        mirror_tree(&directory.join(name), &claude.dir.join(name), false, log)?;
    }
    let keybindings = read_optional(&directory.join(KEYBINDINGS_FILENAME))?;
    sync_file(&claude.keybindings_path(), keybindings.as_deref(), log)?;

    if let Some(settings) = &spec.settings {
        apply_settings(settings, claude, log)?;
    }
    apply_config(&spec, directory, claude, log)?;

    for name in &changes.marketplaces_to_remove {
        let suffix = format!("@{name}");
        for plugin in claude.installed_plugins()? {
            if plugin.ends_with(&suffix) {
                failures.record(cli.plugin_uninstall(&plugin))?;
            }
        }
        failures.record(cli.marketplace_remove(name))?;
    }

    let hidden_root = claude.hidden_root();
    if plugin_specs.is_empty()
        && hidden_root.exists()
        && claude.find_hidden_marketplace()?.is_none()
    {
        fs::remove_dir_all(&hidden_root)
            .with_context(|| format!("removing {}", hidden_root.display()))?;
        log.info(format_args!("removed {}", hidden_root.display()));
    }

    for addition in &changes.marketplaces_to_add {
        if !failures.record(cli.marketplace_add(&addition.source))? {
            continue;
        }
        let Some(marketplace) = claude.find_marketplace_by_source(&addition.source)? else {
            eprintln!(
                "error: failed to find marketplace after adding: {}",
                addition.source
            );
            failures.0 += 1;
            continue;
        };
        let plugins: Vec<String> = addition
            .enabled
            .iter()
            .chain(&addition.disabled)
            .cloned()
            .collect();
        let force = is_local_source(&addition.source)?;
        cli.install_all(&marketplace, &plugins, force, &mut failures)?;
        // An installed plugin starts out disabled.
        for plugin in &addition.enabled {
            failures.record(cli.plugin_enable(&PluginRef::new(plugin, &marketplace)))?;
        }
    }

    let stale: BTreeSet<&str> = changes
        .plugins_to_install
        .iter()
        .map(|plugin| plugin.marketplace.as_str())
        .collect();
    for marketplace in stale {
        failures.record(cli.marketplace_update(marketplace))?;
    }

    for plugin in &changes.plugins_to_remove {
        failures.record(cli.plugin_uninstall(&plugin.to_string()))?;
    }

    let spec_enabled: BTreeSet<&String> = marketplaces
        .iter()
        .flat_map(|entry| &entry.enabled)
        .collect();
    let mut installs: Vec<(String, Vec<String>)> = Vec::new();
    for plugin in &changes.plugins_to_install {
        match installs
            .iter_mut()
            .find(|(marketplace, _)| *marketplace == plugin.marketplace)
        {
            Some((_, plugins)) => plugins.push(plugin.plugin.clone()),
            None => installs.push((plugin.marketplace.clone(), vec![plugin.plugin.clone()])),
        }
    }
    for (marketplace, plugins) in &installs {
        cli.install_all(marketplace, plugins, false, &mut failures)?;
        for plugin in plugins
            .iter()
            .filter(|plugin| spec_enabled.contains(plugin))
        {
            failures.record(cli.plugin_enable(&PluginRef::new(plugin, marketplace)))?;
        }
    }

    for plugin in &changes.plugins_to_enable {
        failures.record(cli.plugin_enable(plugin))?;
    }
    for plugin in &changes.plugins_to_disable {
        failures.record(cli.plugin_disable(plugin))?;
    }
    for plugin in &changes.plugins_to_update {
        let force = claude.is_local_marketplace(&plugin.marketplace)?;
        failures.record(cli.update(plugin, force))?;
    }

    claude.prune_enabled_plugins(log)?;

    println!(
        "{} added, {} removed, {} installed, {} uninstalled, {} enabled, {} disabled, \
         {} updated, {} errors",
        changes.marketplaces_to_add.len(),
        changes.marketplaces_to_remove.len(),
        changes.plugins_to_install.len(),
        changes.plugins_to_remove.len(),
        changes.plugins_to_enable.len(),
        changes.plugins_to_disable.len(),
        changes.plugins_to_update.len(),
        failures.0
    );
    Ok(if failures.0 == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
