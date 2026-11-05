//! Run programs through Proton or Wine in a managed prefix.
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{self, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::builder::ValueParser;
use clap::{ArgAction, ArgMatches, Args, Command as ClapCommand, FromArgMatches, ValueEnum};
use serde::Deserialize;

use crate::tools::Tool;
use crate::{dirs, wayland, which};

const PREFIX_ROOT_VAR: &str = "PROTON_PREFIX_ROOT";
const DEFAULT_PREFIX_ROOT: &str = "~/.local/share/proton-prefixes";
const STEAM_ROOT: &str = ".local/share/Steam";
const SYSTEM_PROTON: &str = "/usr/bin/proton-ge";
const FLATPAK_PROTON: &str = "com.valvesoftware.Steam.CompatibilityTool.Proton-GE";
/// Windows' DPI at 100% scale.
const BASE_DPI: f64 = 96.0;

const PANEL_UPSCALE: &str = "Upscaling / FSR / DLSS";
const PANEL_RENDER: &str = "Rendering / DirectX";
const PANEL_GPU: &str = "GPU / NVAPI";
const PANEL_SYNC: &str = "Sync / Memory";
const PANEL_INPUT: &str = "Input";
const PANEL_MISC: &str = "Drives / Window / Misc";
const PANEL_GAMESCOPE: &str = "Gamescope";

#[derive(Clone, Copy)]
enum Parse {
    Text,
    Integer,
    Float,
}

impl Parse {
    /// Every value is stored as the text it is passed on as.
    fn parser(self) -> ValueParser {
        use clap::builder::TypedValueParser;
        match self {
            Parse::Text => ValueParser::string(),
            Parse::Integer => {
                ValueParser::new(clap::value_parser!(i64).map(|value| value.to_string()))
            }
            Parse::Float => {
                ValueParser::new(|value: &str| value.parse::<f64>().map(|value| value.to_string()))
            }
        }
    }
}

/// An option that sets one environment variable Proton, Wine, or gamescope reads.
enum EnvOption {
    /// `--on` sets the variable to 1, `--off` to 0.
    Toggle {
        on: &'static str,
        off: &'static str,
        var: &'static str,
        heading: &'static str,
        help: &'static str,
    },
    /// `--flag VALUE` sets the variable to VALUE.
    Value {
        flag: &'static str,
        var: &'static str,
        value_name: &'static str,
        parse: Parse,
        heading: &'static str,
        help: &'static str,
    },
}

const fn toggle(
    on: &'static str,
    off: &'static str,
    var: &'static str,
    heading: &'static str,
    help: &'static str,
) -> EnvOption {
    EnvOption::Toggle {
        on,
        off,
        var,
        heading,
        help,
    }
}

const fn value(
    flag: &'static str,
    var: &'static str,
    value_name: &'static str,
    parse: Parse,
    heading: &'static str,
    help: &'static str,
) -> EnvOption {
    EnvOption::Value {
        flag,
        var,
        value_name,
        parse,
        heading,
        help,
    }
}

#[rustfmt::skip]
const ENV_OPTIONS: &[EnvOption] = &[
    toggle("fsr", "no-fsr", "WINE_FULLSCREEN_FSR", PANEL_UPSCALE, "AMD FidelityFX Super Resolution (Vulkan)"),
    value("fsr-strength", "WINE_FULLSCREEN_FSR_STRENGTH", "0-5", Parse::Integer, PANEL_UPSCALE, "FSR sharpening strength (default 2)"),
    value("fsr-mode", "WINE_FULLSCREEN_FSR_CUSTOM_MODE", "WxH", Parse::Text, PANEL_UPSCALE, "FSR fake screen resolution"),
    value("fsr3", "PROTON_FSR3_UPGRADE", "VERSION", Parse::Text, PANEL_UPSCALE, "Inject FSR 3 DLL (version or default)"),
    value("fsr4", "PROTON_FSR4_UPGRADE", "VERSION", Parse::Text, PANEL_UPSCALE, "Inject FSR 4 DLL (version or default 4.0.2)"),
    value("fsr4-rdna3", "PROTON_FSR4_RDNA3_UPGRADE", "VERSION", Parse::Text, PANEL_UPSCALE, "FSR 4 for RDNA3 GPUs (default 4.0.0)"),
    toggle("fsr4-hud", "no-fsr4-hud", "PROTON_FSR4_INDICATOR", PANEL_UPSCALE, "Show FSR 4 watermark overlay"),
    value("dlss", "PROTON_DLSS_UPGRADE", "VERSION", Parse::Text, PANEL_UPSCALE, "Inject NVIDIA DLSS DLL (version or latest)"),
    toggle("dlss-hud", "no-dlss-hud", "PROTON_DLSS_INDICATOR", PANEL_UPSCALE, "Show DLSS overlay indicator"),
    value("xess", "PROTON_XESS_UPGRADE", "VERSION", Parse::Text, PANEL_UPSCALE, "Inject Intel XeSS DLL"),
    toggle("optiscaler", "no-optiscaler", "PROTON_USE_OPTISCALER", PANEL_UPSCALE, "OptiScaler upscaler swap"),
    toggle("wined3d", "no-wined3d", "PROTON_USE_WINED3D", PANEL_RENDER, "OpenGL wined3d instead of DXVK"),
    toggle("wined3d11", "no-wined3d11", "PROTON_USE_WINED3D11", PANEL_RENDER, "wined3d for D3D11 only"),
    toggle("d7vk", "no-d7vk", "PROTON_USE_D7VK", PANEL_RENDER, "d7vk ddraw.dll for old DirectDraw games"),
    toggle("dxvk-d3d8", "no-dxvk-d3d8", "PROTON_DXVK_D3D8", PANEL_RENDER, "D3D8 via DXVK"),
    toggle("no-d3d11", "d3d11", "PROTON_NO_D3D11", PANEL_RENDER, "Disable DirectX 11"),
    toggle("no-d3d10", "d3d10", "PROTON_NO_D3D10", PANEL_RENDER, "Disable DirectX 10"),
    toggle("old-gl-string", "no-old-gl-string", "PROTON_OLD_GL_STRING", PANEL_RENDER, "Limit GL extension string for legacy games"),
    toggle("no-dxgi-device-manager", "dxgi-device-manager", "WINE_DO_NOT_CREATE_DXGI_DEVICE_MANAGER", PANEL_RENDER, "Fix miscolored video playback in some titles"),
    toggle("hide-nvidia", "show-nvidia", "PROTON_HIDE_NVIDIA_GPU", PANEL_GPU, "Hide NVIDIA GPU from the game"),
    toggle("hide-intel", "show-intel", "PROTON_HIDE_INTEL_GPU", PANEL_GPU, "Hide Intel GPU from the game"),
    toggle("hide-apu", "show-apu", "PROTON_HIDE_APU", PANEL_GPU, "Hide APU from the game"),
    toggle("hide-vangogh", "show-vangogh", "PROTON_HIDE_VANGOGH_GPU", PANEL_GPU, "Hide Steam Deck (Van Gogh) GPU"),
    toggle("no-nvapi", "nvapi", "PROTON_DISABLE_NVAPI", PANEL_GPU, "Disable NVIDIA NVAPI"),
    toggle("force-nvapi", "no-force-nvapi", "PROTON_FORCE_NVAPI", PANEL_GPU, "Force-enable NVAPI"),
    toggle("native-ags", "no-native-ags", "PROTON_NATIVE_AGS", PANEL_GPU, "Use native AMD AGS library"),
    toggle("no-esync", "esync", "PROTON_NO_ESYNC", PANEL_SYNC, "Disable eventfd synchronization"),
    toggle("no-fsync", "fsync", "PROTON_NO_FSYNC", PANEL_SYNC, "Disable futex synchronization"),
    toggle("no-ntsync", "ntsync", "PROTON_NO_NTSYNC", PANEL_SYNC, "Disable ntsync kernel module support"),
    toggle("heap-delay-free", "no-heap-delay-free", "PROTON_HEAP_DELAY_FREE", PANEL_SYNC, "Delay heap frees for buggy apps"),
    toggle("heap-zero", "no-heap-zero", "PROTON_HEAP_ZERO_MEMORY", PANEL_SYNC, "Zero heap memory on allocation"),
    toggle("writecopy", "no-writecopy", "PROTON_USE_WRITECOPY", PANEL_SYNC, "Emulate Windows write-copy semantics"),
    toggle("force-lgadd", "no-force-lgadd", "PROTON_FORCE_LARGE_ADDRESS_AWARE", PANEL_SYNC, "Force LARGE_ADDRESS_AWARE on executables"),
    toggle("limit-address-space", "no-limit-address-space", "PROTON_LIMIT_ADDRESS_SPACE", PANEL_SYNC, "Limit address space for buggy apps"),
    toggle("sdl", "no-sdl", "PROTON_USE_SDL", PANEL_INPUT, "Use SDL input instead of hidraw/Steam Input"),
    toggle("prefer-sdl", "no-prefer-sdl", "PROTON_PREFER_SDL", PANEL_INPUT, "Prefer SDL input over hidraw"),
    toggle("no-hidraw", "hidraw", "PROTON_DISABLE_HIDRAW", PANEL_INPUT, "Disable raw HID access"),
    toggle("emulate-steaminput", "no-emulate-steaminput", "PROTON_EMULATE_STEAMINPUT", PANEL_INPUT, "Emulate Steam Input"),
    toggle("no-steaminput", "steaminput", "PROTON_NO_STEAMINPUT", PANEL_INPUT, "Disable Steam Input"),
    toggle("game-drive", "no-game-drive", "PROTON_SET_GAME_DRIVE", PANEL_MISC, "Map the game install as a drive"),
    toggle("steam-drive", "no-steam-drive", "PROTON_SET_STEAM_DRIVE", PANEL_MISC, "Map the Steam library as a drive"),
    toggle("hdr", "no-hdr", "PROTON_ENABLE_HDR", PANEL_MISC, "Enable HDR rendering"),
    toggle("no-wm-decoration", "wm-decoration", "PROTON_NO_WM_DECORATION", PANEL_MISC, "Disable window manager decorations"),
    toggle("no-xim", "xim", "PROTON_NO_XIM", PANEL_MISC, "Disable X input method"),
    toggle("arm64", "no-arm64", "PROTON_USE_ARM64", PANEL_MISC, "Use the ARM64 wine build"),
    value("cpu-topology", "PROTON_CPU_TOPOLOGY", "N:0,1,...", Parse::Text, PANEL_MISC, "Override CPU topology exposed to the game"),
    value("limit-resolutions", "PROTON_LIMIT_RESOLUTIONS", "NUM", Parse::Integer, PANEL_MISC, "Limit number of exposed display resolutions"),
    toggle("no-mediaconv-video", "mediaconv-video", "PROTON_MEDIACONV_NO_VIDEO", PANEL_MISC, "Disable video in the media converter"),
    value("wayland-monitor", "PROTON_WAYLAND_MONITOR", "CONNECTOR", Parse::Text, PANEL_MISC, "Primary Wayland monitor (e.g. eDP-1)"),
    value("log-dir", "PROTON_LOG_DIR", "DIR", Parse::Text, PANEL_MISC, "Directory for Proton logs"),
    value("crash-report-dir", "PROTON_CRASH_REPORT_DIR", "DIR", Parse::Text, PANEL_MISC, "Directory for crash reports"),
    toggle("gamescope-wsi", "no-gamescope-wsi", "ENABLE_GAMESCOPE_WSI", PANEL_GAMESCOPE, "Gamescope WSI layer (disable to fix some games)"),
    toggle("gamescope-tearing", "no-gamescope-tearing", "GAMESCOPE_ALLOW_TEARING", PANEL_GAMESCOPE, "Allow tearing"),
    value("gamescope-limiter-file", "GAMESCOPE_LIMITER_FILE", "FILE", Parse::Text, PANEL_GAMESCOPE, "Path to a live fps-limit file"),
    toggle("dxvk-hdr", "no-dxvk-hdr", "DXVK_HDR", PANEL_GAMESCOPE, "DXVK HDR signalling (pairs with --gamescope-hdr)"),
];

/// A switch passed on to gamescope as `gamescope_flag`.
struct GamescopeSwitch {
    flag: &'static str,
    gamescope_flag: &'static str,
    help: &'static str,
}

/// An option passed on to gamescope as `gamescope_flag VALUE`.
struct GamescopeValue {
    flag: &'static str,
    gamescope_flag: &'static str,
    value_name: &'static str,
    parse: Parse,
    help: &'static str,
}

#[rustfmt::skip]
const GAMESCOPE_SWITCHES: &[GamescopeSwitch] = &[
    GamescopeSwitch { flag: "mangoapp", gamescope_flag: "--mangoapp", help: "MangoHud performance overlay" },
    GamescopeSwitch { flag: "adaptive-sync", gamescope_flag: "--adaptive-sync", help: "Enable variable refresh rate (VRR)" },
    GamescopeSwitch { flag: "immediate-flips", gamescope_flag: "--immediate-flips", help: "Allow tearing via immediate flips" },
    GamescopeSwitch { flag: "force-grab-cursor", gamescope_flag: "--force-grab-cursor", help: "Always use relative mouse mode" },
    GamescopeSwitch { flag: "grab", gamescope_flag: "-g", help: "Grab the keyboard" },
    GamescopeSwitch { flag: "borderless", gamescope_flag: "-b", help: "Borderless window (nested)" },
    GamescopeSwitch { flag: "expose-wayland", gamescope_flag: "--expose-wayland", help: "Support Wayland clients via xdg-shell" },
    GamescopeSwitch { flag: "force-windows-fullscreen", gamescope_flag: "--force-windows-fullscreen", help: "Force windows to nested display size" },
    GamescopeSwitch { flag: "steam-integration", gamescope_flag: "-e", help: "Enable gamescope Steam integration" },
    GamescopeSwitch { flag: "gamescope-hdr", gamescope_flag: "--hdr-enabled", help: "Enable gamescope HDR output" },
    GamescopeSwitch { flag: "hdr-itm", gamescope_flag: "--hdr-itm-enabled", help: "SDR->HDR inverse tone mapping" },
];

#[rustfmt::skip]
const GAMESCOPE_VALUES: &[GamescopeValue] = &[
    GamescopeValue { flag: "scaler", gamescope_flag: "-S", value_name: "MODE", parse: Parse::Text, help: "Upscaler: auto|integer|fit|fill|stretch" },
    GamescopeValue { flag: "filter", gamescope_flag: "-F", value_name: "FILTER", parse: Parse::Text, help: "Upscaler filter: linear|nearest|fsr|nis|pixel" },
    GamescopeValue { flag: "sharpness", gamescope_flag: "--fsr-sharpness", value_name: "0-20", parse: Parse::Integer, help: "FSR/NIS sharpness (0 max .. 20 min)" },
    GamescopeValue { flag: "gs-refresh", gamescope_flag: "-r", value_name: "FPS", parse: Parse::Integer, help: "Gamescope nested refresh rate" },
    GamescopeValue { flag: "mouse-sensitivity", gamescope_flag: "-s", value_name: "FACTOR", parse: Parse::Float, help: "Multiply mouse movement" },
    GamescopeValue { flag: "backend", gamescope_flag: "--backend", value_name: "BACKEND", parse: Parse::Text, help: "auto|drm|sdl|openvr|headless|wayland" },
    GamescopeValue { flag: "prefer-output", gamescope_flag: "-O", value_name: "CONNECTORS", parse: Parse::Text, help: "Preferred output connectors (e.g. DP-1,HDMI-A-1)" },
    GamescopeValue { flag: "orientation", gamescope_flag: "--force-orientation", value_name: "DIR", parse: Parse::Text, help: "Rotate display: normal|left|right|upsidedown" },
    GamescopeValue { flag: "framerate-limit", gamescope_flag: "--framerate-limit", value_name: "FPS", parse: Parse::Integer, help: "Gamescope framerate limit" },
];

impl EnvOption {
    fn augment(&self, command: ClapCommand) -> ClapCommand {
        match *self {
            EnvOption::Toggle {
                on,
                off,
                var,
                heading,
                help,
            } => command
                .arg(
                    clap::Arg::new(on)
                        .long(on)
                        .action(ArgAction::SetTrue)
                        .overrides_with(off)
                        .help(format!("{help} ({var}=1)"))
                        .help_heading(heading),
                )
                .arg(
                    clap::Arg::new(off)
                        .long(off)
                        .action(ArgAction::SetTrue)
                        .overrides_with(on)
                        .help(format!("Set {var}=0"))
                        .help_heading(heading),
                ),
            EnvOption::Value {
                flag,
                var,
                value_name,
                parse,
                heading,
                help,
            } => command.arg(
                clap::Arg::new(flag)
                    .long(flag)
                    .value_name(value_name)
                    .value_parser(parse.parser())
                    .help(format!("{help} ({var})"))
                    .help_heading(heading),
            ),
        }
    }

    /// The variable and value this option sets, if it was given.
    fn setting(&self, matches: &ArgMatches) -> Option<(&'static str, String)> {
        match *self {
            EnvOption::Toggle { on, off, var, .. } => {
                if matches.get_flag(on) {
                    Some((var, "1".into()))
                } else if matches.get_flag(off) {
                    Some((var, "0".into()))
                } else {
                    None
                }
            }
            EnvOption::Value { flag, var, .. } => matches
                .get_one::<String>(flag)
                .map(|value| (var, value.clone())),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Winearch {
    Win32,
    Win64,
}

impl Winearch {
    fn as_str(self) -> &'static str {
        match self {
            Winearch::Win32 => "win32",
            Winearch::Win64 => "win64",
        }
    }
}

/// A screen size written `WxH`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Resolution {
    width: u32,
    height: u32,
}

impl FromStr for Resolution {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parse = || {
            let (width, height) = value.split_once('x')?;
            Some(Resolution {
                width: width.parse().ok()?,
                height: height.parse().ok()?,
            })
        };
        parse().ok_or_else(|| format!("expected WxH, got {value:?}"))
    }
}

/// Where a prefix lives: a path, or a name under the prefix root.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PrefixSpec {
    Path(PathBuf),
    Name(String),
}

impl FromStr for PrefixSpec {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(if value.contains('/') {
            PrefixSpec::Path(value.into())
        } else {
            PrefixSpec::Name(value.to_owned())
        })
    }
}

impl PrefixSpec {
    fn resolve(&self, root: &Path) -> Result<PathBuf> {
        match self {
            PrefixSpec::Path(path) => {
                path::absolute(path).with_context(|| format!("resolving {}", path.display()))
            }
            PrefixSpec::Name(name) => Ok(root.join(name)),
        }
    }
}

#[derive(Args)]
struct Core {
    /// List the installed Proton and Wine versions
    #[arg(long = "versions")]
    show_versions: bool,
    #[arg(long, value_enum, default_value = "win64")]
    winearch: Winearch,
    /// Prefix name under PROTON_PREFIX_ROOT, or a path
    #[arg(short, long, value_name = "PREFIX", default_value = "default")]
    prefix: PrefixSpec,
    /// Proton or Wine version to run with
    #[arg(long = "use", value_name = "VERSION", default_value = "flatpak")]
    use_version: String,
    /// Change to the prefix (handled by the shell integration)
    #[arg(long)]
    cd: bool,
    /// Disable Wayland mode (on by default in a Wayland session)
    #[arg(long)]
    no_wayland: bool,
    /// Sync wine DPI to the KDE primary display scale
    #[arg(long)]
    auto_dpi: bool,
    /// Run through gamescope
    #[arg(long)]
    gamescope: bool,
    /// Gamescope resolution (e.g. 1920x1080)
    #[arg(long, value_name = "WxH")]
    resolution: Option<Resolution>,
    /// Gamescope fullscreen mode
    #[arg(long)]
    fullscreen: bool,
    /// Run in a wine virtual desktop (e.g. 800x600)
    #[arg(long, value_name = "WxH")]
    desktop: Option<Resolution>,
    /// Program and arguments; without them, print the prefix path
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    args: Vec<OsString>,
}

/// `proton`'s arguments: the core options, plus the table-driven environment and gamescope
/// options.
pub struct Proton {
    core: Core,
    env: Vec<(&'static str, String)>,
    gamescope: Vec<String>,
}

impl FromArgMatches for Proton {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let mut gamescope: Vec<String> = GAMESCOPE_SWITCHES
            .iter()
            .filter(|switch| matches.get_flag(switch.flag))
            .map(|switch| switch.gamescope_flag.to_owned())
            .collect();
        for option in GAMESCOPE_VALUES {
            if let Some(value) = matches.get_one::<String>(option.flag) {
                gamescope.extend([option.gamescope_flag.to_owned(), value.clone()]);
            }
        }
        Ok(Proton {
            core: Core::from_arg_matches(matches)?,
            env: ENV_OPTIONS
                .iter()
                .filter_map(|option| option.setting(matches))
                .collect(),
            gamescope,
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Proton::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for Proton {
    fn augment_args(command: ClapCommand) -> ClapCommand {
        let command = Core::augment_args(command)
            .about("Run programs through Proton or Wine in a managed prefix");
        let command = ENV_OPTIONS
            .iter()
            .fold(command, |command, option| option.augment(command));
        let command = GAMESCOPE_SWITCHES.iter().fold(command, |command, switch| {
            command.arg(
                clap::Arg::new(switch.flag)
                    .long(switch.flag)
                    .action(ArgAction::SetTrue)
                    .help(format!("{} ({})", switch.help, switch.gamescope_flag))
                    .help_heading(PANEL_GAMESCOPE),
            )
        });
        GAMESCOPE_VALUES.iter().fold(command, |command, option| {
            command.arg(
                clap::Arg::new(option.flag)
                    .long(option.flag)
                    .value_name(option.value_name)
                    .value_parser(option.parse.parser())
                    .help(format!("{} ({})", option.help, option.gamescope_flag))
                    .help_heading(PANEL_GAMESCOPE),
            )
        })
    }

    fn augment_args_for_update(command: ClapCommand) -> ClapCommand {
        Proton::augment_args(command)
    }
}

impl Tool for Proton {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

/// A program that runs Windows executables: `program args... EXE`.
struct Runner {
    program: PathBuf,
    args: &'static [&'static str],
}

impl Runner {
    fn wine(program: PathBuf) -> Runner {
        Runner { program, args: &[] }
    }

    fn proton(program: PathBuf) -> Runner {
        Runner {
            program,
            args: &["run"],
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(self.args);
        command
    }
}

fn flatpak_proton() -> Option<PathBuf> {
    let output = Command::new("flatpak")
        .args(["info", "--show-location", FLATPAK_PROTON])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let location = String::from_utf8(output.stdout).ok()?;
    Some(PathBuf::from(location.trim()).join("files/proton")).filter(|path| path.is_file())
}

/// The runners installed on this machine, by version name.
fn installed_versions(steam_root: &Path) -> Result<BTreeMap<String, Runner>> {
    let mut versions = BTreeMap::new();
    if let Some(wine) = which::first("wine") {
        versions.insert("wine".to_owned(), Runner::wine(wine));
    }
    if let Some(proton) = flatpak_proton() {
        versions.insert("flatpak".to_owned(), Runner::proton(proton));
    }
    if Path::new(SYSTEM_PROTON).is_file() {
        versions.insert("system".to_owned(), Runner::proton(SYSTEM_PROTON.into()));
    }
    let common = steam_root.join("steamapps/common");
    let entries = match fs::read_dir(&common) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(versions),
        Err(error) => return Err(error).with_context(|| format!("reading {}", common.display())),
    };
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading {}", common.display()))?
            .path();
        let Some(version) = path
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|name| name.strip_prefix("Proton "))
        else {
            continue;
        };
        let proton = path.join("proton");
        if path.is_dir() && proton.is_file() {
            versions.insert(version.trim().to_owned(), Runner::proton(proton));
        }
    }
    Ok(versions)
}

#[derive(Deserialize)]
struct KscreenDoctor {
    outputs: Vec<KscreenOutput>,
}

#[derive(Deserialize)]
struct KscreenOutput {
    #[serde(default)]
    enabled: bool,
    priority: Option<u32>,
    scale: f64,
}

/// The scale of KDE's primary display, when kscreen-doctor is installed and reports one.
fn kde_primary_scale() -> Result<Option<f64>> {
    if which::first("kscreen-doctor").is_none() {
        return Ok(None);
    }
    let output = Command::new("kscreen-doctor")
        .arg("-j")
        .stderr(Stdio::inherit())
        .output()
        .context("running kscreen-doctor")?;
    if !output.status.success() {
        bail!("kscreen-doctor failed: {}", output.status);
    }
    let report: KscreenDoctor =
        serde_json::from_slice(&output.stdout).context("parsing kscreen-doctor's report")?;
    Ok(report
        .outputs
        .into_iter()
        .find(|output| output.enabled && output.priority == Some(1))
        .map(|output| output.scale))
}

enum Casefold {
    Enabled,
    Unsupported,
}

/// Makes `path` case-insensitive (ext4).
///
/// overlayfs cannot be used on a filesystem with casefold enabled:
/// <https://bugzilla.kernel.org/show_bug.cgi?id=216471>
fn casefold(path: &Path) -> Result<Casefold> {
    let output = Command::new("chattr")
        .arg("+F")
        .arg(path)
        .stdout(Stdio::inherit())
        .output()
        .context("running chattr")?;
    if output.status.success() {
        return Ok(Casefold::Enabled);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.starts_with("chattr: Operation not supported while setting flags on ") {
        return Ok(Casefold::Unsupported);
    }
    bail!("chattr failed ({}): {}", output.status, stderr.trim_end());
}

/// The directory to run in and the executable for a `C:\...` path inside the prefix.
fn windows_path(drive_c: &Path, arg: &str) -> Option<(PathBuf, PathBuf)> {
    let rest = arg
        .get(..3)
        .filter(|drive| drive.eq_ignore_ascii_case("c:\\"))
        .map(|_| &arg[3..])?;
    let mut parts: Vec<&str> = rest.split('\\').collect();
    let exe = parts.pop().unwrap_or_default();
    let dir = parts
        .into_iter()
        .fold(drive_c.to_owned(), |dir, part| dir.join(part));
    let exe = dir.join(exe);
    Some((dir, exe))
}

/// `path` with a leading `~` replaced by `home`.
fn expand_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some("") => home.to_owned(),
        Some(rest) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => path.into(),
    }
}

fn gamescope_args(proton: &Proton) -> Option<Vec<String>> {
    let core = &proton.core;
    if !(core.gamescope
        || core.resolution.is_some()
        || core.fullscreen
        || !proton.gamescope.is_empty())
    {
        return None;
    }
    let mut args = Vec::new();
    if let Some(Resolution { width, height }) = core.resolution {
        let (width, height) = (width.to_string(), height.to_string());
        args.extend(["-w", &width, "-h", &height, "-W", &width, "-H", &height].map(str::to_owned));
    }
    if core.fullscreen {
        args.push("-f".to_owned());
    }
    args.extend(proton.gamescope.iter().cloned());
    Some(args)
}

fn run(proton: Proton) -> Result<ExitCode> {
    let core = &proton.core;
    if core.cd {
        bail!("cd zsh integration failure");
    }
    let home = dirs::home()?;
    let prefix_root = env::var(PREFIX_ROOT_VAR).unwrap_or_else(|_| DEFAULT_PREFIX_ROOT.to_owned());
    let prefix = core.prefix.resolve(&expand_home(&prefix_root, &home))?;
    let steam_root = home.join(STEAM_ROOT);

    let versions = installed_versions(&steam_root)?;
    if core.show_versions {
        for version in versions.keys() {
            println!("{version}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let runner = versions.get(&core.use_version).with_context(|| {
        let known: Vec<&str> = versions.keys().map(String::as_str).collect();
        format!(
            "no such version: {} (installed: {})",
            core.use_version,
            known.join(", ")
        )
    })?;

    let Some((first, rest)) = core.args.split_first() else {
        if !prefix.is_dir() {
            bail!("no such prefix: {}", prefix.display());
        }
        println!("{}", prefix.display());
        return Ok(ExitCode::SUCCESS);
    };

    if !prefix.is_dir() {
        fs::create_dir_all(&prefix).with_context(|| format!("creating {}", prefix.display()))?;
        if let Casefold::Unsupported = casefold(&prefix)? {
            println!("Casefold not supported. Ignoring");
        }
    }

    let mut cwd = None;
    let mut program_args: Vec<OsString> = core.args.clone();
    if let Some((dir, exe)) = first
        .to_str()
        .and_then(|first| windows_path(&prefix.join("pfx/drive_c"), first))
    {
        if !dir.is_dir() {
            bail!("no such directory: {}", dir.display());
        }
        cwd = Some(dir);
        program_args = std::iter::once(exe.into_os_string())
            .chain(rest.iter().cloned())
            .collect();
    }
    if let Some(Resolution { width, height }) = core.desktop {
        program_args.splice(
            0..0,
            [
                OsString::from("explorer"),
                OsString::from(format!("/desktop=proton,{width}x{height}")),
            ],
        );
    }

    let wayland = wayland::current_display().is_some() && !core.no_wayland;
    let mut env: Vec<(OsString, OsString)> = vec![
        (
            "STEAMAPPS_COMMON".into(),
            steam_root.join("steamapps/common").into(),
        ),
        (
            "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
            steam_root.clone().into(),
        ),
        ("STEAM_COMPAT_DATA_PATH".into(), prefix.clone().into()),
        ("WINEARCH".into(), core.winearch.as_str().into()),
        ("WINEPREFIX".into(), prefix.join("pfx").into()),
        (
            "PROTON_ENABLE_WAYLAND".into(),
            if wayland { "1" } else { "0" }.into(),
        ),
    ];
    env.extend(
        proton
            .env
            .iter()
            .map(|(var, value)| (var.into(), value.into())),
    );

    if core.auto_dpi
        && let Some(scale) = kde_primary_scale()?
    {
        let dpi = (BASE_DPI * scale).round_ties_even();
        let mut command = runner.command();
        command
            .args([
                "reg",
                "add",
                "HKCU\\Control Panel\\Desktop",
                "/v",
                "LogPixels",
            ])
            .args(["/t", "REG_DWORD", "/d", &dpi.to_string(), "/f"])
            .envs(env.iter().map(|(k, v)| (k, v)));
        if let Some(cwd) = &cwd {
            command.current_dir(cwd);
        }
        let status = command.status().context("setting the wine DPI")?;
        if !status.success() {
            bail!("setting the wine DPI failed: {status}");
        }
    }

    let mut command = match gamescope_args(&proton) {
        Some(args) => {
            let mut command = Command::new("gamescope");
            command
                .args(args)
                .arg("--")
                .arg(&runner.program)
                .args(runner.args);
            command
        }
        None => runner.command(),
    };
    command
        .args(&program_args)
        .envs(env.iter().map(|(k, v)| (k, v)));
    if let Some(cwd) = &cwd {
        command.current_dir(cwd);
    }
    Err(command.exec()).with_context(|| format!("running {}", command.get_program().display()))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    #[command(version = "test")]
    struct Cli {
        #[command(flatten)]
        proton: Proton,
    }

    #[test]
    fn arguments_are_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Proton {
        Cli::try_parse_from(std::iter::once("proton").chain(args.iter().copied()))
            .unwrap()
            .proton
    }

    #[test]
    fn toggles_and_values_set_environment() {
        let proton = parse(&[
            "--fsr",
            "--fsr-strength",
            "3",
            "--esync",
            "--no-d3d11",
            "--dlss",
            "latest",
            "game.exe",
        ]);
        assert_eq!(
            proton.env,
            [
                ("WINE_FULLSCREEN_FSR", "1".to_owned()),
                ("WINE_FULLSCREEN_FSR_STRENGTH", "3".to_owned()),
                ("PROTON_DLSS_UPGRADE", "latest".to_owned()),
                ("PROTON_NO_D3D11", "1".to_owned()),
                ("PROTON_NO_ESYNC", "0".to_owned()),
            ]
        );
        assert_eq!(proton.core.args, ["game.exe"]);
    }

    #[test]
    fn later_toggle_wins() {
        let proton = parse(&["--fsr", "--no-fsr"]);
        assert_eq!(proton.env, [("WINE_FULLSCREEN_FSR", "0".to_owned())]);
    }

    #[test]
    fn integer_values_are_checked() {
        let result = Cli::try_parse_from(["proton", "--fsr-strength", "high"]);
        assert!(result.is_err());
    }

    #[test]
    fn program_arguments_are_left_alone() {
        let proton = parse(&["game.exe", "--fsr", "-windowed"]);
        assert_eq!(proton.core.args, ["game.exe", "--fsr", "-windowed"]);
        assert!(proton.env.is_empty());
    }

    #[test]
    fn gamescope_runs_when_any_gamescope_option_is_given() {
        assert_eq!(gamescope_args(&parse(&[])), None);
        assert_eq!(gamescope_args(&parse(&["--gamescope"])), Some(vec![]));
        assert_eq!(
            gamescope_args(&parse(&[
                "--resolution",
                "1920x1080",
                "--fullscreen",
                "--grab",
                "--mangoapp",
                "--sharpness",
                "5",
                "--mouse-sensitivity",
                "1.5",
            ])),
            Some(
                [
                    "-w",
                    "1920",
                    "-h",
                    "1080",
                    "-W",
                    "1920",
                    "-H",
                    "1080",
                    "-f",
                    "--mangoapp",
                    "-g",
                    "--fsr-sharpness",
                    "5",
                    "-s",
                    "1.5",
                ]
                .map(str::to_owned)
                .to_vec()
            )
        );
    }

    #[test]
    fn resolutions_parse() {
        assert_eq!(
            "800x600".parse(),
            Ok(Resolution {
                width: 800,
                height: 600
            })
        );
        assert!("800".parse::<Resolution>().is_err());
        assert!("800xabc".parse::<Resolution>().is_err());
    }

    #[test]
    fn prefixes_are_names_or_paths() {
        let root = Path::new("/prefixes");
        assert_eq!(
            "game".parse::<PrefixSpec>().unwrap().resolve(root).unwrap(),
            root.join("game")
        );
        assert_eq!(
            "/abs/pfx"
                .parse::<PrefixSpec>()
                .unwrap()
                .resolve(root)
                .unwrap(),
            Path::new("/abs/pfx")
        );
    }

    #[test]
    fn windows_paths_map_into_drive_c() {
        let drive_c = Path::new("/pfx/drive_c");
        assert_eq!(
            windows_path(drive_c, "C:\\Games\\Foo\\foo.exe"),
            Some((drive_c.join("Games/Foo"), drive_c.join("Games/Foo/foo.exe")))
        );
        assert_eq!(
            windows_path(drive_c, "c:\\foo.exe"),
            Some((drive_c.to_owned(), drive_c.join("foo.exe")))
        );
        assert_eq!(windows_path(drive_c, "foo.exe"), None);
        assert_eq!(windows_path(drive_c, "D:\\foo.exe"), None);
    }

    #[test]
    fn home_is_expanded() {
        let home = Path::new("/home/u");
        assert_eq!(expand_home("~/x", home), home.join("x"));
        assert_eq!(expand_home("~", home), home);
        assert_eq!(expand_home("/abs", home), Path::new("/abs"));
    }
}
