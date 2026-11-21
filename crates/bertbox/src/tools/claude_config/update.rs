//! `update`: refresh every registered marketplace and installed plugin.
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::tools::claude_config::files::Log;
use crate::tools::claude_config::plugins::{ClaudeCli, Failures};
use crate::tools::claude_config::spec::PluginRef;
use crate::tools::claude_config::state::ClaudeDir;

pub fn run(claude: &ClaudeDir, log: Log) -> Result<ExitCode> {
    let cli = ClaudeCli::new(claude);
    let mut failures = Failures::default();
    claude.prune_enabled_plugins(log)?;

    let known = claude.known_marketplaces()?;
    let mut marketplaces: Vec<&str> = known.names().collect();
    marketplaces.sort_by_key(|name| name.to_lowercase());
    for name in &marketplaces {
        log.info(format_args!("marketplace: {name}"));
        failures.record(cli.marketplace_update(name))?;
    }

    let mut plugins = claude.installed_plugins()?;
    plugins.sort_by_key(|plugin| plugin.to_lowercase());
    for installed in &plugins {
        let plugin = PluginRef::parse(installed)
            .with_context(|| format!("installed plugin {installed:?} names no marketplace"))?;
        log.info(format_args!("plugin: {plugin}"));
        let force = claude.is_local_marketplace(&plugin.marketplace)?;
        failures.record(cli.update(&plugin, force))?;
    }

    println!(
        "{} marketplaces, {} plugins, {} errors",
        marketplaces.len(),
        plugins.len(),
        failures.0
    );
    Ok(if failures.0 == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
