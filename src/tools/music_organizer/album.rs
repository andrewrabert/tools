use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tools::music_organizer::files::{extension, set_permissions};
use crate::tools::music_organizer::log::Log;
use crate::tools::music_organizer::name::{folder_name, safe_rename, title_name};
use crate::tools::music_organizer::tags::{AUDIO_EXTS, Field, Tags};

const PADDED_TRACK_LIMIT: u32 = 10;

pub struct Options<'a> {
    pub scan_root: &'a Path,
    pub organize_dirs: Option<&'a Path>,
    pub library: Option<&'a Path>,
    pub dry_run: bool,
}

/// What one album directory's tracks say about it.
struct Album {
    tracks: Vec<PathBuf>,
    artists: BTreeSet<String>,
    albumartists: BTreeSet<String>,
    albums: BTreeSet<String>,
    missing_album: bool,
    missing_artist: bool,
}

/// Fix the tags and names inside `dir`, then place `dir` itself; returns the artists seen.
pub fn organize(log: &mut Log, dir: &Path, options: &Options) -> Result<BTreeSet<String>> {
    let album = scan(log, dir, options.dry_run)?;
    let mut dir = dir.to_path_buf();

    if album.albums.len() > 1 {
        log.error(format!("Multiple album tags in \"{}\"", dir.display()));
    } else if let Some(album_tag) = album.albums.iter().next() {
        let expected_album = folder_name(album_tag)
            .filter(|name| !name.is_empty())
            .with_context(|| format!("invalid album name \"{}\"", dir.display()))?;
        if let Some(organize_dirs) = options.organize_dirs {
            let placed = place(log, &dir, &album, &expected_album, organize_dirs, options)?;
            let Some(placed) = placed else {
                return Ok(album.artists);
            };
            dir = placed;
        }
    }

    if album.albumartists.len() > 1 {
        log.error(format!("Multiple albumartists in \"{}\"", dir.display()));
    } else if !album.missing_artist
        && !album.missing_album
        && album.albumartists.is_empty()
        && album.artists.len() == 1
        && album.albums.len() == 1
    {
        let artist = album.artists.iter().next().context("no artist")?;
        for track in &album.tracks {
            log.info(format!(
                "Setting albumartist to \"{artist}\" ({})",
                track.display()
            ));
            if !options.dry_run {
                let mut tags = Tags::read(track)?;
                tags.set(Field::AlbumArtist, vec![artist.clone()]);
                tags.save(track)?;
            }
        }
    }

    if let (Some(library), [albumartist]) = (
        options.library,
        album.albumartists.iter().collect::<Vec<_>>().as_slice(),
    ) {
        let relative = dir
            .strip_prefix(library)
            .with_context(|| format!("{} is outside the library", dir.display()))?;
        let top_level = folder_name(albumartist)
            .with_context(|| format!("invalid albumartist \"{albumartist}\""))?;
        let expected: PathBuf = Path::new(&top_level)
            .iter()
            .chain(relative.iter().skip(1))
            .collect();
        if expected != relative {
            log.error(format!(
                "top-level should be \"{}\" for \"{}\"",
                expected.display(),
                relative.display()
            ));
        }
    }

    Ok(album.artists)
}

fn scan(log: &mut Log, dir: &Path, dry_run: bool) -> Result<Album> {
    let mut album = Album {
        tracks: Vec::new(),
        artists: BTreeSet::new(),
        albumartists: BTreeSet::new(),
        albums: BTreeSet::new(),
        missing_album: false,
        missing_artist: false,
    };
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("reading {}", dir.display()))?;
    entries.sort();
    for path in entries {
        if path.is_symlink() {
            log.error(format!("Symlink detected \"{}\"", path.display()));
            continue;
        }
        if path.is_dir() {
            continue;
        }
        set_permissions(&path)?;
        if !AUDIO_EXTS.contains(&extension(&path)) {
            continue;
        }
        let mut tags =
            Tags::read(&path).with_context(|| format!("reading the tags of {}", path.display()))?;
        for field in Field::EXPECTED {
            if !tags.has(field) {
                log.error(format!(
                    "File {} missing \"{}\"",
                    path.display(),
                    field.name()
                ));
            }
        }
        let mut changed = fix_metadata(log, &path, &mut tags);
        changed |= fix_tracknumber(log, &path, &mut tags);
        let renamed_to = rename_from_tags(log, &path, &tags);

        if let Some(album_tag) = tags.first(Field::Album) {
            album.albums.insert(album_tag.to_owned());
        } else {
            album.missing_album = true;
        }
        if tags.has(Field::Artist) {
            album
                .artists
                .extend(tags.get(Field::Artist).map(str::to_owned));
        } else {
            album.missing_artist = true;
        }
        album
            .artists
            .extend(tags.get(Field::AlbumArtist).map(str::to_owned));
        album
            .albumartists
            .extend(tags.get(Field::AlbumArtist).map(str::to_owned));

        if changed && !dry_run {
            tags.save(&path)
                .with_context(|| format!("writing the tags of {}", path.display()))?;
        }
        let path = match renamed_to {
            Some(name) => safe_rename(log, &path, &name, dry_run),
            None => path,
        };
        album.tracks.push(path);
    }
    Ok(album)
}

fn fix_metadata(log: &mut Log, path: &Path, tags: &mut Tags) -> bool {
    let normalized = tags.normalize();
    for key in &normalized.fixed_keys {
        log.info(format!(
            "Fixing whitespace/unicode normalization in \"{key}\": \"{}\"",
            path.display()
        ));
    }
    for key in &normalized.unprintable_keys {
        log.error(format!(
            "Non-printable character detected in \"{key}\": \"{}\"",
            path.display()
        ));
    }
    !normalized.fixed_keys.is_empty()
}

fn fix_tracknumber(log: &mut Log, path: &Path, tags: &mut Tags) -> bool {
    let mut changed = tags.remove_track_total();
    if let Some(tracknumber) = tags.first(Field::TrackNumber)
        && let Some((number, _total)) = tracknumber.split_once('/')
    {
        let number = number.to_owned();
        if number.parse::<u32>().is_err() {
            log.error(format!("Unhandled tracknumber: \"{number}\""));
        }
        tags.set(Field::TrackNumber, vec![number]);
        changed = true;
    }
    if changed {
        log.info(format!("Fixing track number: \"{}\"", path.display()));
    }
    changed
}

/// The file name the track's tags call for, if they can name it.
fn rename_from_tags(log: &mut Log, path: &Path, tags: &Tags) -> Option<String> {
    let title = tags.first(Field::Title)?;
    let Some(tracknumber) = tags
        .first(Field::TrackNumber)
        .and_then(|tracknumber| tracknumber.parse::<u32>().ok())
    else {
        log.error(format!("Cannot rename \"{}\"", path.display()));
        return None;
    };
    let tracknumber = if tracknumber < PADDED_TRACK_LIMIT {
        format!("{tracknumber:02}")
    } else {
        tracknumber.to_string()
    };
    Some(format!(
        "{tracknumber} {}.{}",
        title_name(title),
        extension(path)
    ))
}

/// Move or rename `dir` to where its tags say it belongs; `None` when a clash stops the work.
fn place(
    log: &mut Log,
    dir: &Path,
    album: &Album,
    expected_album: &str,
    organize_dirs: &Path,
    options: &Options,
) -> Result<Option<PathBuf>> {
    let parent_name = dir.parent().and_then(Path::file_name);
    let expected_artist = match album.albumartists.iter().collect::<Vec<_>>().as_slice() {
        [albumartist] => folder_name(albumartist).filter(|name| !name.is_empty()),
        _ => None,
    };
    match expected_artist {
        Some(expected_artist) if parent_name != Some(OsStr::new(&expected_artist)) => {
            let artist_dir = organize_dirs.join(&expected_artist);
            let album_dir = artist_dir.join(expected_album);
            if album_dir.exists() {
                log.error(format!(
                    "cannot move album \"{}\" to \"{}\": target already exists",
                    dir.display(),
                    album_dir.display()
                ));
                return Ok(Some(dir.to_path_buf()));
            }
            if dir == options.scan_root {
                log.info(format!(
                    "moving files from \"{}\" to \"{}\"",
                    dir.display(),
                    album_dir.display()
                ));
                if options.dry_run {
                    return Ok(Some(dir.to_path_buf()));
                }
                fs::create_dir_all(&album_dir)
                    .with_context(|| format!("creating {}", album_dir.display()))?;
                for entry in
                    fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?
                {
                    let path = entry
                        .with_context(|| format!("reading {}", dir.display()))?
                        .path();
                    if path.is_file() {
                        let name = path
                            .file_name()
                            .with_context(|| format!("{} has no name", path.display()))?;
                        fs::rename(&path, album_dir.join(name))
                            .with_context(|| format!("moving {}", path.display()))?;
                    }
                }
            } else {
                log.info(format!(
                    "moving album from \"{}\" to \"{}\"",
                    dir.display(),
                    album_dir.display()
                ));
                if options.dry_run {
                    return Ok(Some(dir.to_path_buf()));
                }
                fs::create_dir_all(&artist_dir)
                    .with_context(|| format!("creating {}", artist_dir.display()))?;
                fs::rename(dir, &album_dir).with_context(|| format!("moving {}", dir.display()))?;
            }
            Ok(Some(album_dir))
        }
        _ => {
            let name = dir.file_name().and_then(|name| name.to_str());
            if name == Some(expected_album) {
                return Ok(Some(dir.to_path_buf()));
            }
            log.info(format!(
                "renaming album folder from \"{}\" to \"{expected_album}\"",
                dir.display()
            ));
            let new_dir = dir.with_file_name(expected_album);
            if new_dir.exists() {
                log.error(format!("new root already exists \"{}\"", new_dir.display()));
                return Ok(None);
            }
            if options.dry_run {
                return Ok(Some(dir.to_path_buf()));
            }
            fs::rename(dir, &new_dir).with_context(|| format!("renaming {}", dir.display()))?;
            Ok(Some(new_dir))
        }
    }
}
