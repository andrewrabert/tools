use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn install_bundle(path: &Path) -> Result<()> {
    println!("Installing...");
    let status = Command::new("flatpak")
        .args(["install", "--user", "--noninteractive", "--or-update"])
        .arg(path)
        .status()
        .context("running flatpak")?;
    if !status.success() {
        bail!("flatpak install failed: {status}");
    }
    println!("Installed");
    Ok(())
}
