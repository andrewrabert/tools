use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use tempfile::TempPath;

use crate::tools::archive::pool;
use crate::tools::img::cli::{Convert, Kernel, ResizeMode};
use crate::tools::img::encode::{self, Settings};
use crate::tools::img::mime::{self, Mime};
use crate::tools::img::{exif, jobs, probe, require_file, svg, temp};

const TEMP_PREFIX: &str = ".imgconvert_";

/// Where outputs go: the source's directory by default.
struct Output {
    dir: Option<PathBuf>,
    name: Option<OsString>,
}

pub struct Options {
    pub max_w: Option<u32>,
    pub max_h: Option<u32>,
    pub scale: Option<f64>,
    pub kernel: Option<Kernel>,
    pub resize_mode: ResizeMode,
    pub force: bool,
    pub settings: Settings,
}

enum Job {
    Whole {
        path: PathBuf,
        source: Mime,
        target: Mime,
    },
    Page {
        path: PathBuf,
        page: u32,
        target: Mime,
        name: OsString,
    },
}

impl Job {
    fn source(&self) -> &Path {
        match self {
            Job::Whole { path, .. } | Job::Page { path, .. } => path,
        }
    }
}

pub fn run(args: Convert) -> Result<ExitCode> {
    let has_resize = args.width.is_some()
        || args.height.is_some()
        || args.scale.is_some()
        || args.filter.is_some();
    if args.format.is_none() && !has_resize {
        bail!("-f/--format is required when no resize options are given");
    }
    if args.scale.is_some() && (args.width.is_some() || args.height.is_some()) {
        bail!("--scale cannot be used with --width or --height");
    }
    for path in &args.paths {
        require_file(path)?;
    }

    let output = match &args.output {
        None => Output {
            dir: None,
            name: None,
        },
        Some(path) if path.is_dir() => Output {
            dir: Some(path.clone()),
            name: None,
        },
        Some(path) => {
            let dir = path
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            if !dir.is_dir() {
                bail!("output parent directory does not exist: {}", dir.display());
            }
            if args.paths.len() > 1 && !args.combine {
                bail!("-o as a filename requires exactly one input file");
            }
            Output {
                dir: Some(dir.to_path_buf()),
                name: path.file_name().map(OsStr::to_owned),
            }
        }
    };

    let options = Options {
        max_w: args.width.map(|w| w.get()),
        max_h: args.height.map(|h| h.get()),
        scale: args.scale,
        kernel: args.filter,
        resize_mode: args.resize_mode,
        force: args.force,
        settings: Settings {
            jpeg_quality: args.jpeg_quality,
            jxl_distance: if args.jxl_lossy {
                Settings::LOSSY_DISTANCE
            } else {
                Settings::LOSSLESS_DISTANCE
            },
            jxl_effort: if args.fast {
                Settings::FAST_EFFORT
            } else {
                Settings::FULL_EFFORT
            },
        },
    };

    if args.combine {
        return combine(&args, &output);
    }

    // How many outputs each source still owes, so --rm only removes it once
    // every page derived from it has been written.
    let mut remaining: HashMap<PathBuf, usize> = HashMap::new();
    let mut jobs_to_run = Vec::new();
    for path in &args.paths {
        let detected = match mime::detect(path) {
            Ok(detected) => detected,
            // A broken symlink, most likely.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        let Some(detected) = detected else {
            bail!("unrecognized image: {}", path.display());
        };
        let target = args.format.map_or(detected.mime, |format| format.mime());

        let pages = if detected.mime == Mime::Tiff {
            probe::tiff_pages(path)?
        } else {
            1
        };
        if pages > 1 {
            if output.name.is_some() {
                bail!(
                    "cannot write a multi-page TIFF to a single -o filename; pass a directory instead"
                );
            }
            let width = pages.to_string().len();
            let stem = path.file_stem().unwrap_or_default();
            remaining.insert(path.clone(), pages as usize);
            for page in 0..pages {
                let mut name = stem.to_owned();
                name.push(format!("_{:0width$}{}", page + 1, target.suffix()));
                jobs_to_run.push(Job::Page {
                    path: path.clone(),
                    page,
                    target,
                    name,
                });
            }
            continue;
        }
        remaining.insert(path.clone(), 1);
        jobs_to_run.push(Job::Whole {
            path: path.clone(),
            source: detected.mime,
            target,
        });
    }

    let mut failed = false;
    pool::run(
        jobs_to_run,
        jobs(args.num_procs)?,
        |job| match job {
            Job::Whole {
                path,
                source,
                target,
            } => convert(
                path,
                *source,
                *target,
                output.dir.as_deref(),
                output.name.as_deref(),
                &options,
            ),
            Job::Page {
                path,
                page,
                target,
                name,
            } => convert_page(path, *page, *target, output.dir.as_deref(), name, &options),
        },
        |job, result| {
            let source = job.source();
            let dest = match result {
                Ok(dest) => dest,
                Err(error) => {
                    failed = true;
                    eprintln!("Error: {error:#}");
                    return;
                }
            };
            println!("{} -> {}", source.display(), dest.display());
            let owed = remaining.get_mut(source).map(|owed| {
                *owed = owed.saturating_sub(1);
                *owed
            });
            if args.rm && source != dest && owed.unwrap_or(0) == 0 {
                remove_if_present(source);
            }
        },
    );
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn remove_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("Error: removing {}: {error}", path.display()),
    }
}

fn combine(args: &Convert, output: &Output) -> Result<ExitCode> {
    let target = args.format.map(|format| format.mime());
    let Some(target) = target.filter(|target| target.is_multipage()) else {
        bail!("--combine requires a multi-page output format (-f tiff)");
    };
    let (Some(dir), Some(name)) = (&output.dir, &output.name) else {
        bail!("--combine requires -o with a filename");
    };
    let name = with_suffix(name.as_bytes(), target);
    let dest = dir.join(&name);

    if !args.force && dest.exists() {
        for path in &args.paths {
            if same_file(&dest, path)? {
                bail!(
                    "destination is source (use --force to overwrite): {}",
                    dest.display()
                );
            }
        }
        bail!(
            "output already exists (use --force to overwrite): {}",
            dest.display()
        );
    }

    let tmp = temp::file(dir, ".imgconvert_combine_", &name)?;
    encode::combine(&args.paths, &tmp)?;
    temp::replace(tmp, &dest)?;

    let sources: Vec<String> = args
        .paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    println!("{} -> {}", sources.join(", "), dest.display());

    if args.rm {
        for path in &args.paths {
            if *path != dest {
                remove_if_present(path);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Converts one page of a multi-page TIFF by way of a PNG.
fn convert_page(
    source: &Path,
    page: u32,
    target: Mime,
    output_dir: Option<&Path>,
    name: &OsStr,
    options: &Options,
) -> Result<PathBuf> {
    let dest_dir = output_dir.unwrap_or_else(|| parent(source));
    let page_png = temp::with_suffix(dest_dir, ".imgconvert_page_", ".png")?;
    encode::extract_tiff_page(source, page, &page_png)?;
    convert(
        &page_png,
        Mime::Png,
        target,
        Some(dest_dir),
        Some(name),
        options,
    )
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// The output name: the source name with its own suffix swapped for the target's.
fn target_name(path: &Path, source: Mime, target: Mime) -> OsString {
    let full = path.file_name().unwrap_or_default();
    let stem = path.file_stem().unwrap_or_default();
    let extension = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy().to_ascii_lowercase()));
    let base = match extension {
        Some(ext) if source.suffixes().contains(&ext.as_str()) => stem,
        _ => full,
    };
    with_suffix(base.as_bytes(), target)
}

fn with_suffix(name: &[u8], target: Mime) -> OsString {
    let has_suffix = target.suffixes().iter().any(|suffix| {
        name.len() >= suffix.len()
            && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
    });
    let mut name = name.to_vec();
    if !has_suffix {
        name.extend_from_slice(target.suffix().as_bytes());
    }
    OsString::from_vec(name)
}

fn same_file(a: &Path, b: &Path) -> Result<bool> {
    let a = fs::metadata(a).with_context(|| format!("stat {}", a.display()))?;
    let b = fs::metadata(b).with_context(|| format!("stat {}", b.display()))?;
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

fn scaled(size: u32, scale: f64) -> u32 {
    (f64::from(size) * scale).round() as u32
}

fn needs_resize(
    (img_w, img_h): (u32, u32),
    max_w: Option<u32>,
    max_h: Option<u32>,
    mode: ResizeMode,
) -> bool {
    let target_w = max_w.unwrap_or(img_w);
    let target_h = max_h.unwrap_or(img_h);
    match mode {
        ResizeMode::Shrink => img_w > target_w || img_h > target_h,
        ResizeMode::Grow => img_w < target_w || img_h < target_h,
        ResizeMode::Stretch => img_w != target_w || img_h != target_h,
    }
}

/// A Krita document flattened to a PNG, or any other source as it is.
fn flattened(path: &Path, source: Mime, dest_dir: &Path) -> Result<(PathBuf, Option<TempPath>)> {
    if source != Mime::Krita {
        return Ok((path.to_path_buf(), None));
    }
    let png = temp::with_suffix(dest_dir, TEMP_PREFIX, ".png")?;
    encode::from_kra(path, &png)?;
    Ok((png.to_path_buf(), Some(png)))
}

/// Converts `path` and returns the written destination.
pub fn convert(
    path: &Path,
    source: Mime,
    target: Mime,
    output_dir: Option<&Path>,
    output_name: Option<&OsStr>,
    options: &Options,
) -> Result<PathBuf> {
    let original = path;
    let name = output_name.map_or_else(|| target_name(path, source, target), OsStr::to_owned);

    match source {
        Mime::Gif if probe::is_animated_gif(path)? => {
            bail!("gif is animated: {}", path.display())
        }
        Mime::Heif if probe::heif_frames(path)? != 1 => {
            bail!("heif must contain exactly one frame: {}", path.display())
        }
        _ => {}
    }

    let (mut max_w, mut max_h, mut mode) = (options.max_w, options.max_h, options.resize_mode);
    if let Some(scale) = options.scale
        && source != Mime::Svg
    {
        let (width, height) = probe::dimensions(path)?;
        max_w = Some(scaled(width, scale));
        max_h = Some(scaled(height, scale));
        mode = ResizeMode::Stretch;
    }

    let dest_dir = output_dir.unwrap_or_else(|| parent(path));
    let dest = dest_dir.join(&name);
    if !options.force && dest.exists() {
        if same_file(&dest, path)? {
            bail!(
                "destination is source (use --force to overwrite): {}",
                path.display()
            );
        }
        bail!(
            "output already exists (use --force to overwrite): {}",
            dest.display()
        );
    }

    // Resize first into a PNG, and convert that instead.
    let resized;
    let mut source = source;
    let mut path = path;
    if (max_w.is_some() || max_h.is_some()) && !matches!(source, Mime::Krita | Mime::Svg) {
        let dimensions = probe::dimensions(path)?;
        if needs_resize(dimensions, max_w, max_h, mode) {
            resized = temp::with_suffix(dest_dir, ".imgconvert_resize_", ".png")?;
            encode::resize(
                path,
                &resized,
                max_w.or(max_h).unwrap_or(dimensions.0),
                max_h,
                mode,
                options.kernel,
                options.scale,
            )?;
            path = &*resized;
            source = Mime::Png;
        }
    }

    let settings = &options.settings;
    let tmp = temp::sibling(dest_dir, TEMP_PREFIX, path)?;
    let png_temp = || temp::with_suffix(dest_dir, TEMP_PREFIX, ".png");
    let produced: TempPath = match (target, source) {
        (Mime::Png, Mime::Svg) => {
            let png = png_temp()?;
            svg::to_png(path, &png, max_w, max_h, options.scale)?;
            png
        }
        (Mime::Png, Mime::Krita) => {
            let png = png_temp()?;
            encode::from_kra(path, &png)?;
            png
        }
        (Mime::Png, _) => {
            encode::to_png(path, &tmp)?;
            tmp
        }
        (Mime::Bmp, _) => {
            let (input, _flat) = flattened(path, source, dest_dir)?;
            encode::to_bmp(&input, &tmp)?;
            tmp
        }
        (Mime::Ppm, _) => {
            let (input, _flat) = flattened(path, source, dest_dir)?;
            encode::to_ppm(&input, &tmp)?;
            tmp
        }
        (Mime::Tiff, Mime::Jxl) => {
            encode::to_tiff_vips(path, &tmp)?;
            exif::copy_tiff_tags(path, &tmp)?;
            tmp
        }
        (Mime::Tiff, _) => {
            let (input, _flat) = flattened(path, source, dest_dir)?;
            encode::to_tiff_vips(&input, &tmp)?;
            tmp
        }
        (Mime::Jxl, Mime::Heif) => {
            let ppm = temp::sibling(dest_dir, TEMP_PREFIX, path)?;
            encode::to_ppm(path, &ppm)?;
            encode::to_jxl(&ppm, &tmp, settings)?;
            tmp
        }
        (Mime::Jxl, Mime::Tiff) => {
            encode::to_jxl_vips(path, &tmp)?;
            exif::copy_tiff_tags(path, &tmp)?;
            tmp
        }
        (Mime::Jxl, _) => {
            let (input, _flat) = flattened(path, source, dest_dir)?;
            encode::to_jxl(&input, &tmp, settings)?;
            tmp
        }
        (Mime::Jpeg, _) => {
            let produced = match source {
                Mime::Svg => {
                    let png = png_temp()?;
                    svg::to_png(path, &png, max_w, max_h, options.scale)?;
                    encode::to_jpeg(&png, &tmp, settings)?;
                    tmp
                }
                Mime::Heif | Mime::Tiff | Mime::Webp | Mime::Jpeg => {
                    let ppm = temp::sibling(dest_dir, TEMP_PREFIX, path)?;
                    encode::to_ppm(path, &ppm)?;
                    encode::to_jpeg(&ppm, &tmp, settings)?;
                    tmp
                }
                Mime::Jxl => {
                    let mut suffix = path.file_name().map(OsStr::to_owned).unwrap_or_default();
                    suffix.push(".jpg");
                    let jpg = temp::file(dest_dir, TEMP_PREFIX, &suffix)?;
                    encode::djxl(path, &jpg)?;
                    jpg
                }
                Mime::Krita => {
                    let png = png_temp()?;
                    encode::from_kra(path, &png)?;
                    encode::to_jpeg(&png, &tmp, settings)?;
                    tmp
                }
                _ => {
                    encode::to_jpeg(path, &tmp, settings)?;
                    tmp
                }
            };
            if source.carries_jpeg_metadata() {
                exif::copy_metadata(original, &produced)?;
            }
            produced
        }
        (other, _) => bail!("unsupported output format: {}", other.as_str()),
    };
    temp::replace(produced, &dest)?;
    Ok(dest)
}
