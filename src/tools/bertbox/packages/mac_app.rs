use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

const APPLICATIONS: &str = "/Applications";
const QUARANTINE: &str = "com.apple.quarantine";

pub fn install_bundle(dmg: &Path, app: &str) -> Result<()> {
    let mount = dmg.with_file_name("mnt");
    fs::create_dir_all(&mount).with_context(|| format!("creating {}", mount.display()))?;
    run(Command::new("diskutil")
        .args(["image", "attach", "--nobrowse", "--mountPoint"])
        .arg(&mount)
        .arg(dmg))?;
    let dest = Path::new(APPLICATIONS).join(app);
    let copied = copy(&mount.join(app), &dest);
    let ejected = run(Command::new("diskutil").arg("eject").arg(&mount));
    copied?;
    ejected?;

    if quarantined(&dest)? {
        println!("Clearing quarantine (sudo)...");
        run(Command::new("sudo")
            .args(["xattr", "-drs", QUARANTINE])
            .arg(&dest))?;
    }
    Ok(())
}

fn copy(source: &Path, dest: &Path) -> Result<()> {
    if !source.is_dir() {
        let name = source.file_name().unwrap_or_default().to_string_lossy();
        bail!("{name} not found in the disk image");
    }
    let incoming = version(source)?;
    println!("Version: {incoming}");
    let installed = if dest.exists() {
        Some(version(dest)?)
    } else {
        None
    };
    if installed.as_deref() == Some(incoming.as_str()) {
        println!("Already installed: {}", dest.display());
        return Ok(());
    }
    if let Some(installed) = installed {
        println!("Replacing installed version {installed}");
        fs::remove_dir_all(dest).with_context(|| format!("removing {}", dest.display()))?;
    }
    println!("Installing to {}...", dest.display());
    run(Command::new("ditto").arg(source).arg(dest))?;
    println!("Installed: {}", dest.display());
    Ok(())
}

fn version(app: &Path) -> Result<String> {
    let info = app.join("Contents/Info.plist");
    let short = plist_value(&info, "CFBundleShortVersionString")?;
    let build = plist_value(&info, "CFBundleVersion")?;
    Ok(match (short, build) {
        (Some(short), Some(build)) if short != build => format!("{short} ({build})"),
        (Some(short), _) => short,
        (None, Some(build)) => build,
        (None, None) => "unknown".to_owned(),
    })
}

fn plist_value(path: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(path)
        .stderr(Stdio::null())
        .output()
        .context("running plutil")?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

fn quarantined(app: &Path) -> Result<bool> {
    let output = Command::new("xattr")
        .arg("-rs")
        .arg(app)
        .output()
        .context("running xattr")?;
    if !output.status.success() {
        bail!("xattr -rs {} failed: {}", app.display(), output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains(QUARANTINE))
}

fn run(command: &mut Command) -> Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .with_context(|| format!("running {program}"))?;
    if !status.success() {
        bail!("{program} failed: {status}");
    }
    Ok(())
}
