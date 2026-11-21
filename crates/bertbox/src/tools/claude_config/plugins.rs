//! Marketplace and plugin changes, and the `claude plugin` commands that make them.
use std::collections::BTreeSet;
use std::fmt;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::tools::claude_config::paths::{local_path, normalize_source, resolve};
use crate::tools::claude_config::spec::{MarketplaceEntry, PluginRef};
use crate::tools::claude_config::state::{ClaudeDir, MarketplaceState};

#[derive(Debug)]
pub struct CommandFailed(String);

impl fmt::Display for CommandFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandFailed {}

/// Counts the `claude plugin` commands that failed, so a run carries on past them.
#[derive(Default)]
pub struct Failures(pub usize);

impl Failures {
    pub fn record(&mut self, result: Result<()>) -> Result<bool> {
        let Err(error) = result else {
            return Ok(true);
        };
        let failure = error.downcast::<CommandFailed>()?;
        eprintln!("error: {failure}");
        self.0 += 1;
        Ok(false)
    }
}

/// Runs `claude plugin` from the config directory, so a command never acts on the caller's repo.
pub struct ClaudeCli<'a> {
    claude: &'a ClaudeDir,
}

impl<'a> ClaudeCli<'a> {
    pub fn new(claude: &'a ClaudeDir) -> Self {
        Self { claude }
    }

    fn run(&self, args: &[&str]) -> Result<()> {
        let output = Command::new("claude")
            .arg("plugin")
            .args(args)
            .current_dir(&self.claude.dir)
            .stdin(Stdio::null())
            .output()
            .context("running claude")?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = match stderr.trim_end() {
            "" => "unknown error",
            message => message,
        };
        Err(CommandFailed(message.to_owned()).into())
    }

    fn run_tolerating(&self, args: &[&str], tolerated: &str) -> Result<()> {
        match self.run(args) {
            Err(error)
                if error
                    .downcast_ref::<CommandFailed>()
                    .is_some_and(|failure| failure.0.contains(tolerated)) =>
            {
                Ok(())
            }
            result => result,
        }
    }

    pub fn marketplace_add(&self, source: &str) -> Result<()> {
        match local_path(source)? {
            Some(local) => {
                let resolved = resolve(&local)?.display().to_string();
                self.run(&["marketplace", "add", &resolved])
            }
            None => self.run(&["marketplace", "add", source]),
        }
    }

    pub fn marketplace_update(&self, name: &str) -> Result<()> {
        self.run(&["marketplace", "update", name])
    }

    pub fn marketplace_remove(&self, name: &str) -> Result<()> {
        self.run(&["marketplace", "remove", name])
    }

    fn plugin_install(&self, plugin: &str) -> Result<()> {
        self.run(&["install", plugin])
    }

    fn plugin_update(&self, plugin: &str) -> Result<()> {
        self.run(&["update", plugin])
    }

    pub fn plugin_uninstall(&self, plugin: &str) -> Result<()> {
        self.run(&["uninstall", plugin])
    }

    pub fn plugin_enable(&self, plugin: &PluginRef) -> Result<()> {
        self.run_tolerating(&["enable", &plugin.to_string()], "already enabled")
    }

    pub fn plugin_disable(&self, plugin: &PluginRef) -> Result<()> {
        self.run_tolerating(&["disable", &plugin.to_string()], "already disabled")
    }

    pub fn update(&self, plugin: &PluginRef, force: bool) -> Result<()> {
        let name = plugin.to_string();
        if force {
            self.plugin_uninstall(&name)?;
            self.claude.clear_plugin_cache(plugin)?;
            self.plugin_install(&name)
        } else {
            self.plugin_update(&name)
        }
    }

    /// Installs a plugin, falling back to updating it when the install fails. `force` uninstalls
    /// it and empties its cache first.
    fn install_or_update(&self, plugin: &PluginRef, force: bool) -> Result<()> {
        let name = plugin.to_string();
        let install = || {
            if force {
                self.plugin_uninstall(&name)?;
                self.claude.clear_plugin_cache(plugin)?;
            }
            self.plugin_install(&name)
        };
        match install() {
            Err(error) if error.is::<CommandFailed>() => self.plugin_update(&name),
            result => result,
        }
    }

    pub fn install_all(
        &self,
        marketplace: &str,
        plugins: &[String],
        force: bool,
        failures: &mut Failures,
    ) -> Result<()> {
        for plugin in plugins {
            let plugin = PluginRef::new(plugin, marketplace);
            let result = self
                .install_or_update(&plugin, force)
                .map_err(|error| match error.downcast::<CommandFailed>() {
                    Ok(failure) => CommandFailed(format!("{plugin}: {failure}")).into(),
                    Err(error) => error,
                });
            failures.record(result)?;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub struct Addition {
    pub source: String,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

#[derive(Default, Debug, PartialEq)]
pub struct Changes {
    pub marketplaces_to_add: Vec<Addition>,
    pub marketplaces_to_remove: Vec<String>,
    pub plugins_to_install: Vec<PluginRef>,
    pub plugins_to_remove: Vec<PluginRef>,
    pub plugins_to_enable: Vec<PluginRef>,
    pub plugins_to_disable: Vec<PluginRef>,
    pub plugins_to_update: Vec<PluginRef>,
}

fn upsert<V>(items: &mut Vec<(String, V)>, key: String, value: V) {
    match items.iter_mut().find(|(existing, _)| *existing == key) {
        Some(item) => item.1 = value,
        None => items.push((key, value)),
    }
}

impl Changes {
    pub fn compute(
        current: &[(String, MarketplaceState)],
        marketplaces: &[MarketplaceEntry],
    ) -> Result<Self> {
        let mut desired: Vec<(String, &MarketplaceEntry)> = Vec::new();
        for entry in marketplaces {
            upsert(&mut desired, normalize_source(&entry.source)?, entry);
        }
        let mut current_by_source: Vec<(String, (&str, &MarketplaceState))> = Vec::new();
        for (name, state) in current {
            let source = normalize_source(&state.source)?;
            upsert(&mut current_by_source, source, (name.as_str(), state));
        }

        let mut changes = Changes::default();
        for (source, (name, _)) in &current_by_source {
            if !desired.iter().any(|(wanted, _)| wanted == source) {
                changes.marketplaces_to_remove.push((*name).to_owned());
            }
        }

        for (source, entry) in desired {
            let registered = current_by_source
                .iter()
                .find_map(|(current, found)| (*current == source).then_some(*found));
            let Some((marketplace, state)) = registered else {
                changes.marketplaces_to_add.push(Addition {
                    source: entry.source.clone(),
                    enabled: entry.enabled.iter().cloned().collect(),
                    disabled: entry.disabled.iter().cloned().collect(),
                });
                continue;
            };
            let wanted = entry.installed();
            let installed = state.installed();
            let remaining: BTreeSet<&String> = wanted.intersection(&installed).collect();
            let plugin = |plugin: &String| PluginRef::new(plugin, marketplace);

            let install = wanted.difference(&installed).map(plugin);
            changes.plugins_to_install.extend(install);
            let remove = installed.difference(&wanted).map(plugin);
            changes.plugins_to_remove.extend(remove);
            let enable = remaining
                .iter()
                .filter(|name| entry.enabled.contains(**name) && !state.enabled.contains(**name))
                .map(|name| plugin(name));
            changes.plugins_to_enable.extend(enable);
            let disable = remaining
                .iter()
                .filter(|name| entry.disabled.contains(**name) && !state.disabled.contains(**name))
                .map(|name| plugin(name));
            changes.plugins_to_disable.extend(disable);
            let update = remaining.iter().map(|name| plugin(name));
            changes.plugins_to_update.extend(update);
        }
        Ok(changes)
    }
}

/// Whether a marketplace source is a local path, whose plugins are reinstalled rather than updated.
pub fn is_local_source(source: &str) -> Result<bool> {
    Ok(local_path(source)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::claude_config::spec::MarketplaceType;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn entry(source: &str, enabled: &[&str], disabled: &[&str]) -> MarketplaceEntry {
        MarketplaceEntry::new(
            None,
            MarketplaceType::Github,
            source.to_owned(),
            set(enabled),
            set(disabled),
            BTreeSet::new(),
        )
        .unwrap()
    }

    fn state(source: &str, enabled: &[&str], disabled: &[&str]) -> MarketplaceState {
        MarketplaceState {
            source: source.to_owned(),
            source_type: "github".to_owned(),
            enabled: set(enabled),
            disabled: set(disabled),
        }
    }

    fn refs(plugins: &[&str], marketplace: &str) -> Vec<PluginRef> {
        plugins
            .iter()
            .map(|plugin| PluginRef::new(plugin, marketplace))
            .collect()
    }

    #[test]
    fn unregistered_marketplaces_are_added_and_undeclared_ones_removed() {
        let current = [("old".to_owned(), state("o/old", &["a"], &[]))];
        let changes = Changes::compute(&current, &[entry("o/new", &["a"], &["b"])]).unwrap();
        assert_eq!(
            changes,
            Changes {
                marketplaces_to_add: vec![Addition {
                    source: "o/new".to_owned(),
                    enabled: vec!["a".to_owned()],
                    disabled: vec!["b".to_owned()],
                }],
                marketplaces_to_remove: vec!["old".to_owned()],
                ..Changes::default()
            }
        );
    }

    #[test]
    fn plugins_of_a_registered_marketplace_are_reconciled() {
        let current = [(
            "m".to_owned(),
            state("o/r", &["stays", "to-disable", "gone"], &["to-enable"]),
        )];
        let wanted = entry("o/r", &["stays", "to-enable", "new"], &["to-disable"]);
        let changes = Changes::compute(&current, &[wanted]).unwrap();
        assert_eq!(
            changes,
            Changes {
                plugins_to_install: refs(&["new"], "m"),
                plugins_to_remove: refs(&["gone"], "m"),
                plugins_to_enable: refs(&["to-enable"], "m"),
                plugins_to_disable: refs(&["to-disable"], "m"),
                plugins_to_update: refs(&["stays", "to-disable", "to-enable"], "m"),
                ..Changes::default()
            }
        );
    }

    #[test]
    fn local_sources_match_by_resolved_path() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().display().to_string();
        let current = [("m".to_owned(), state(&dir, &[], &[]))];
        let wanted = entry(&format!("{dir}/sub/.."), &[], &[]);
        assert_eq!(
            Changes::compute(&current, &[wanted]).unwrap(),
            Changes::default()
        );
    }

    #[test]
    fn failures_count_failed_commands_and_pass_other_errors_on() {
        let mut failures = Failures::default();
        assert!(failures.record(Ok(())).unwrap());
        assert!(
            !failures
                .record(Err(CommandFailed("x".to_owned()).into()))
                .unwrap()
        );
        assert_eq!(failures.0, 1);
        assert!(failures.record(Err(anyhow::anyhow!("other"))).is_err());
        assert_eq!(failures.0, 1);
    }
}
