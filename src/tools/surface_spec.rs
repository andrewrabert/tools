mod python;
mod rust;

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::slice;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args as ClapArgs, ValueEnum};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::{Walk, WalkBuilder};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, Value, context};
use serde::de::{self, DeserializeOwned, MapAccess, Visitor};
use serde::ser;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::atomic_write;
use crate::git::Git;
use crate::tools::Tool;

const CHANGE: &str = include_str!("surface_spec/templates/surface_spec_change.md.j2");
const SURFACE_SPEC: &str = include_str!("surface_spec/templates/surface_spec.md.j2");
const COMMON: &str = include_str!("surface_spec/templates/common.md.j2");

#[derive(ClapArgs)]
#[command(
    about = "Render the declarations in Rust and Python source files as a surface spec, or as the additions and deletions of a change"
)]
pub struct SurfaceSpec {
    /// A source file or directory to list every declaration of; with --git, one to limit the
    /// change to [default with --git: .]
    #[arg(
        value_name = "PATH",
        required_unless_present_any = ["additions", "deletions", "git"],
        conflicts_with_all = ["additions", "deletions"],
    )]
    paths: Vec<PathBuf>,
    /// List the declarations of this file or directory as additions; with --deletions, list
    /// those it adds to that one, pairing files by their path under each directory
    #[arg(short, long, value_name = "PATH")]
    additions: Option<PathBuf>,
    /// List the declarations of this file or directory as deletions; with --additions, list
    /// those that one deletes from it
    #[arg(short, long, value_name = "PATH")]
    deletions: Option<PathBuf>,
    /// List the declarations a git diff adds and deletes: `REV` from REV to the working tree,
    /// `A..B` from A to B, or `A...B` from their merge base to B; an omitted side is HEAD
    #[arg(
        short,
        long,
        value_name = "REVISIONS",
        conflicts_with_all = ["additions", "deletions", "input_json"],
    )]
    git: Option<GitDiff>,
    /// Read each path as a JSON capture printed by --json instead of as sources
    #[arg(long)]
    input_json: bool,
    /// Print the declarations of PATH as JSON syntax trees instead of a Markdown surface spec
    #[arg(
        long,
        conflicts_with_all = [
            "additions",
            "deletions",
            "git",
            "language",
            "include_rust_private",
            "include_rust_tests",
        ],
    )]
    json: bool,
    #[command(flatten)]
    filter: Filter,
    #[command(flatten)]
    rust: rust::Includes,
    /// Render the spec for this language's files [default: every language captured]
    #[arg(short, long, value_enum)]
    language: Option<Language>,
    /// Write the JSON to this file, or each language's spec to `<language>.md` in this
    /// directory, instead of stdout; required to render more than one language
    #[arg(short, long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[derive(Clone)]
enum GitDiff {
    WorkingTree(String),
    Revisions { base: String, head: String },
    MergeBase { base: String, head: String },
}

impl FromStr for GitDiff {
    type Err = Infallible;

    fn from_str(revisions: &str) -> Result<Self, Self::Err> {
        let side = |revision: &str| {
            if revision.is_empty() {
                "HEAD"
            } else {
                revision
            }
            .to_owned()
        };
        Ok(if let Some((base, head)) = revisions.split_once("...") {
            Self::MergeBase {
                base: side(base),
                head: side(head),
            }
        } else if let Some((base, head)) = revisions.split_once("..") {
            Self::Revisions {
                base: side(base),
                head: side(head),
            }
        } else {
            Self::WorkingTree(revisions.to_owned())
        })
    }
}

impl GitDiff {
    fn changes(
        &self,
        paths: &[PathBuf],
        selection: &Selection,
    ) -> Result<(Vec<FileChange>, Vec<anyhow::Error>)> {
        let git = Git::cwd();
        let (base, head) = match self {
            Self::WorkingTree(base) => (base.clone(), working_tree(&git, paths, selection)?),
            Self::Revisions { base, head } => (base.clone(), tree(&git, head, paths, selection)?),
            Self::MergeBase { base, head } => (
                git.capture(["merge-base", base, head])?
                    .trim_end()
                    .to_owned(),
                tree(&git, head, paths, selection)?,
            ),
        };
        Ok(FileChange::pair(tree(&git, &base, paths, selection)?, head))
    }
}

fn listed_sources(listed: &str, selection: &Selection) -> BTreeMap<PathBuf, Language> {
    listed
        .split_terminator('\0')
        .map(PathBuf::from)
        .filter(|path| selection.keeps(path))
        .filter_map(|path| Language::of(&path).map(|language| (path, language)))
        .collect()
}

fn pathspec(paths: &[PathBuf]) -> impl Iterator<Item = OsString> {
    [OsString::from("--")]
        .into_iter()
        .chain(paths.iter().map(OsString::from))
}

fn tree(
    git: &Git,
    revision: &str,
    paths: &[PathBuf],
    selection: &Selection,
) -> Result<BTreeMap<PathBuf, Result<FileSurface>>> {
    let listed = git.capture(
        ["ls-tree", "-r", "-z", "--name-only", revision]
            .map(OsString::from)
            .into_iter()
            .chain(pathspec(paths)),
    )?;
    Ok(listed_sources(&listed, selection)
        .into_iter()
        .map(|(path, language)| {
            let object = format!("{revision}:./{}", path.display());
            let surface = git
                .capture(["cat-file", "blob", object.as_str()])
                .and_then(|source| FileSurface::parse(path.clone(), language, &source));
            (path, surface)
        })
        .collect())
}

fn working_tree(
    git: &Git,
    paths: &[PathBuf],
    selection: &Selection,
) -> Result<BTreeMap<PathBuf, Result<FileSurface>>> {
    let mut ls_files = vec!["ls-files", "-z", "--cached", "--others"];
    if selection.gitignore {
        ls_files.push("--exclude-standard");
    }
    let listed = git.capture(
        ls_files
            .into_iter()
            .map(OsString::from)
            .chain(pathspec(paths)),
    )?;
    Ok(listed_sources(&listed, selection)
        .into_iter()
        // A file in the index but gone from the working tree is deleted.
        .filter(|(path, _)| path.is_file())
        .map(|(path, language)| (path.clone(), FileSurface::read(path, language)))
        .collect())
}

#[derive(ClapArgs)]
struct Filter {
    /// Keep only files matching this gitignore-style glob; repeatable
    #[arg(short, long = "include", value_name = "PATTERN")]
    includes: Vec<String>,
    /// Drop files and directories matching this gitignore-style glob; repeatable
    #[arg(short, long = "exclude", value_name = "PATTERN")]
    excludes: Vec<String>,
    /// Search directories without skipping what .gitignore ignores
    #[arg(short = 'I', long)]
    no_ignore: bool,
}

impl Filter {
    fn select(&self) -> Result<Selection> {
        let cwd = env::current_dir().context("reading the current directory")?;
        let mut builder = OverrideBuilder::new(cwd);
        let excludes = self.excludes.iter().map(|pattern| format!("!{pattern}"));
        for pattern in self.includes.iter().cloned().chain(excludes) {
            builder
                .add(&pattern)
                .with_context(|| format!("parsing pattern {pattern}"))?;
        }
        Ok(Selection {
            overrides: builder.build()?,
            gitignore: !self.no_ignore,
        })
    }
}

struct Selection {
    overrides: Override,
    gitignore: bool,
}

impl Selection {
    fn keeps(&self, path: &Path) -> bool {
        !self.overrides.matched(path, false).is_ignore()
            && !path
                .ancestors()
                .skip(1)
                .filter(|dir| !dir.as_os_str().is_empty())
                .any(|dir| self.overrides.matched(dir, true).is_ignore())
    }

    fn walk(&self, root: &Path) -> Walk {
        let mut builder = WalkBuilder::new(root);
        builder
            .standard_filters(false)
            .follow_links(true)
            .overrides(self.overrides.clone())
            .parents(self.gitignore)
            .git_ignore(self.gitignore)
            .git_global(self.gitignore)
            .git_exclude(self.gitignore);
        if self.gitignore {
            builder.filter_entry(|entry| entry.file_name() != ".git");
        }
        builder.build()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Language {
    Rust,
    Python,
}

impl Language {
    fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            _ => None,
        }
    }

    fn template(self) -> &'static str {
        match self {
            Self::Rust => include_str!("surface_spec/templates/rust.md.j2"),
            Self::Python => include_str!("surface_spec/templates/python.md.j2"),
        }
    }

    fn spec_file(self) -> &'static str {
        match self {
            Self::Rust => "rust.md",
            Self::Python => "python.md",
        }
    }
}

impl Tool for SurfaceSpec {
    fn run(self) -> ExitCode {
        match self.print() {
            Ok(()) => ExitCode::SUCCESS,
            Err(Errors(errors)) => {
                for error in &errors {
                    eprintln!("Error: {error:?}");
                }
                ExitCode::FAILURE
            }
        }
    }
}

struct Errors(Vec<anyhow::Error>);

impl From<anyhow::Error> for Errors {
    fn from(error: anyhow::Error) -> Self {
        Self(vec![error])
    }
}

impl Errors {
    fn check(errors: Vec<anyhow::Error>) -> Result<(), Self> {
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Self(errors))
        }
    }
}

impl SurfaceSpec {
    fn print(self) -> Result<(), Errors> {
        let selection = self.filter.select()?;
        let (changes, mut errors) = match (&self.git, &self.additions, &self.deletions) {
            (Some(diff), _, _) => {
                let paths = match self.paths.as_slice() {
                    [] => vec![PathBuf::from(".")],
                    paths => paths.to_vec(),
                };
                diff.changes(&paths, &selection)?
            }
            (None, Some(additions), Some(deletions)) => {
                let (base, mut errors) = side(deletions, self.input_json, &selection);
                let (head, head_errors) = side(additions, self.input_json, &selection);
                let (changes, pair_errors) = FileChange::pair(base, head);
                errors.extend(head_errors);
                errors.extend(pair_errors);
                (changes, errors)
            }
            (None, Some(additions), None) => {
                let (files, errors) = load(slice::from_ref(additions), self.input_json, &selection);
                (files.into_iter().map(FileChange::Added).collect(), errors)
            }
            (None, None, Some(deletions)) => {
                let (files, errors) = load(slice::from_ref(deletions), self.input_json, &selection);
                (files.into_iter().map(FileChange::Deleted).collect(), errors)
            }
            (None, None, None) => return self.print_surface(&selection),
        };
        let languages = match self.language {
            Some(language) => BTreeSet::from([language]),
            None => changes.iter().map(FileChange::language).collect(),
        };
        match write_specs(
            languages,
            |language| FileChange::spec(&changes, language, &self.rust),
            self.output.as_deref(),
        ) {
            Ok(spec_errors) | Err(Errors(spec_errors)) => errors.extend(spec_errors),
        }
        Errors::check(errors)
    }

    fn print_surface(&self, selection: &Selection) -> Result<(), Errors> {
        let (files, mut errors) = load(&self.paths, self.input_json, selection);
        let surface = CapturedSurface { files };
        if self.json {
            if let Err(error) = write_capture(&surface, self.output.as_deref()) {
                errors.push(error);
            }
            return Errors::check(errors);
        }
        let languages = match self.language {
            Some(language) => BTreeSet::from([language]),
            None => surface.languages(),
        };
        match write_specs(
            languages,
            |language| surface.spec(language, &self.rust),
            self.output.as_deref(),
        ) {
            Ok(spec_errors) | Err(Errors(spec_errors)) => errors.extend(spec_errors),
        }
        Errors::check(errors)
    }
}

fn load(
    paths: &[PathBuf],
    input_json: bool,
    selection: &Selection,
) -> (Vec<FileSurface>, Vec<anyhow::Error>) {
    if !input_json {
        return split(read_sources(paths, selection));
    }
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match CapturedSurface::load(path) {
            Ok(surface) => files.extend(
                surface
                    .files
                    .into_iter()
                    .filter(|file| selection.keeps(file.path())),
            ),
            Err(error) => errors.push(error),
        }
    }
    (files, errors)
}

fn side(
    root: &Path,
    input_json: bool,
    selection: &Selection,
) -> (BTreeMap<PathBuf, Result<FileSurface>>, Vec<anyhow::Error>) {
    if input_json {
        let (files, errors) = load(&[root.to_path_buf()], true, selection);
        let files = files
            .into_iter()
            .map(|file| (file.path().to_path_buf(), Ok(file)))
            .collect();
        return (files, errors);
    }
    let (sources, errors) = split(sources(&[root.to_path_buf()], selection));
    let files = sources
        .into_iter()
        .map(|(path, language)| {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let listed = if relative.as_os_str().is_empty() {
                path.clone()
            } else {
                relative.clone()
            };
            let surface = fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))
                .and_then(|source| FileSurface::parse(listed, language, &source));
            (relative, surface)
        })
        .collect();
    (files, errors)
}

fn write_capture(surface: &CapturedSurface, output: Option<&Path>) -> Result<()> {
    let json = serde_json::to_string(surface)?;
    match output {
        Some(path) => write(path, &format!("{json}\n"))?,
        None => writeln!(io::stdout(), "{json}").context("writing to stdout")?,
    }
    Ok(())
}

/// Writes the spec `spec` renders for each of `languages`. `Ok` holds the errors of files left
/// out of the written specs; `Err` holds those followed by the error that stopped the write.
fn write_specs(
    languages: BTreeSet<Language>,
    spec: impl Fn(Language) -> Result<(String, Vec<anyhow::Error>)>,
    output: Option<&Path>,
) -> Result<Vec<anyhow::Error>, Errors> {
    match (output, Vec::from_iter(languages).as_slice()) {
        (_, []) => {
            Err(anyhow!("no files to render; pass --language to render an empty spec").into())
        }
        (None, [language]) => {
            let (spec, errors) = spec(*language)?;
            write!(io::stdout(), "{spec}").context("writing to stdout")?;
            Ok(errors)
        }
        (None, _) => Err(anyhow!(
            "the surface has both Rust and Python files; pass --output to write a spec for each"
        )
        .into()),
        (Some(dir), languages) => {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            let mut errors = Vec::new();
            for &language in languages {
                let (spec, spec_errors) = match spec(language) {
                    Ok(spec) => spec,
                    Err(error) => {
                        errors.push(error);
                        return Err(Errors(errors));
                    }
                };
                errors.extend(spec_errors);
                if let Err(error) = write(&dir.join(language.spec_file()), &spec) {
                    errors.push(error);
                    return Err(Errors(errors));
                }
            }
            Ok(errors)
        }
    }
}

fn split<T>(results: impl IntoIterator<Item = Result<T>>) -> (Vec<T>, Vec<anyhow::Error>) {
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok(value) => values.push(value),
            Err(error) => errors.push(error),
        }
    }
    (values, errors)
}

fn write(path: &Path, contents: &str) -> Result<()> {
    let temp = atomic_write::sibling(".surface-spec-", path)?;
    fs::write(&temp, contents).with_context(|| format!("writing {}", temp.display()))?;
    atomic_write::replace(temp, path)
}

fn read_sources(paths: &[PathBuf], selection: &Selection) -> Vec<Result<FileSurface>> {
    sources(paths, selection)
        .into_iter()
        .map(|source| source.and_then(|(path, language)| FileSurface::read(path, language)))
        .collect()
}

fn sources(paths: &[PathBuf], selection: &Selection) -> Vec<Result<(PathBuf, Language)>> {
    let mut sources = BTreeMap::new();
    let mut errors = Vec::new();
    for path in paths {
        for entry in selection.walk(path) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(error.into());
                    continue;
                }
            };
            let is_file = entry.file_type().is_some_and(|kind| kind.is_file());
            if !is_file || (entry.depth() == 0 && !selection.keeps(entry.path())) {
                continue;
            }
            match Language::of(entry.path()) {
                Some(language) => {
                    sources.insert(entry.into_path(), language);
                }
                None if entry.depth() == 0 => errors.push(anyhow!(
                    "capturing {}: not a Rust (.rs) or Python (.py, .pyi) source file",
                    entry.path().display()
                )),
                None => {}
            }
        }
    }
    errors
        .into_iter()
        .map(Err)
        .chain(sources.into_iter().map(Ok))
        .collect()
}

struct CapturedSurface {
    files: Vec<FileSurface>,
}

/// A capture as JSON: every node by its id, beside the id of the root, the node of the path that
/// contains every captured file. Each node lists the ids of the nodes it encloses: the path's node
/// and a directory's node the directories and files in it on the way to a captured file, a file's
/// node its top-level declarations.
#[derive(Deserialize)]
struct CaptureJson {
    root: CaptureId,
    #[serde(deserialize_with = "unique_nodes")]
    nodes: BTreeMap<CaptureId, serde_json::Value>,
}

/// A node's id in a capture's JSON, unique across the capture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct CaptureId(usize);

/// In source order.
#[derive(Deserialize)]
struct Children {
    #[serde(default)]
    children: Vec<CaptureId>,
}

/// Whether a node's JSON is the path's node, a directory's node, a file's node, or a
/// declaration's.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum NodeKind {
    Path(PathJson),
    Dir(DirJson),
    File(FileJson),
    #[serde(other)]
    Declaration,
}

/// The root's node: the directory that contains every captured file, as given.
#[derive(Deserialize)]
struct PathJson {
    path: PathBuf,
}

/// A directory's node, named within the path or directory enclosing it.
#[derive(Deserialize)]
struct DirJson {
    name: Name,
}

/// A file's node, named within the path or directory enclosing it.
#[derive(Deserialize)]
struct FileJson {
    language: Language,
    name: Name,
}

/// One segment of a path. A `Name` can't be empty, `.`, `..`, a root or a prefix, and it never
/// contains a separator. It is UTF-8 so that it fits in a JSON string.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
struct Name(String);

/// Why a string or path component is not a `Name`.
#[derive(Debug)]
enum NameError {
    /// It is not exactly one normal component: it is empty, `.`, `..`, a root, a prefix, or
    /// more than one segment.
    NotOneSegment(String),
    NotUtf8,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotOneSegment(name) => write!(f, "{name:?} is not a single path segment"),
            Self::NotUtf8 => f.write_str("path segment is not UTF-8"),
        }
    }
}

impl std::error::Error for NameError {}

impl TryFrom<Component<'_>> for Name {
    type Error = NameError;

    fn try_from(component: Component<'_>) -> Result<Self, NameError> {
        match component {
            Component::Normal(segment) => segment
                .to_str()
                .map(|segment| Self(segment.to_owned()))
                .ok_or(NameError::NotUtf8),
            other => Err(NameError::NotOneSegment(
                other.as_os_str().to_string_lossy().into_owned(),
            )),
        }
    }
}

impl TryFrom<String> for Name {
    type Error = NameError;

    fn try_from(name: String) -> Result<Self, NameError> {
        let mut components = Path::new(&name).components();
        match (components.next(), components.next()) {
            // Against the whole string, since `components` reads "a/" and "a/." as `Normal("a")`.
            (Some(Component::Normal(segment)), None) if segment == name.as_str() => Ok(Self(name)),
            _ => Err(NameError::NotOneSegment(name)),
        }
    }
}

impl AsRef<Path> for Name {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

#[derive(Serialize)]
struct NodeJson<'a> {
    #[serde(flatten)]
    node: AnyNode<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<CaptureId>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum AnyNode<'a> {
    Path {
        kind: &'static str,
        path: &'a Path,
    },
    Dir {
        kind: &'static str,
        name: &'a Name,
    },
    File {
        kind: &'static str,
        language: Language,
        name: &'a Name,
    },
    Rust(&'a rust::Node),
    Python(&'a python::Node),
}

fn unique_nodes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<CaptureId, serde_json::Value>, D::Error> {
    struct Nodes;

    impl<'de> Visitor<'de> for Nodes {
        type Value = BTreeMap<CaptureId, serde_json::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a map of node ids to nodes")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut nodes = BTreeMap::new();
            while let Some((id, node)) = map.next_entry::<CaptureId, serde_json::Value>()? {
                if nodes.insert(id, node).is_some() {
                    return Err(de::Error::custom(format!(
                        "node {} appears more than once",
                        id.0
                    )));
                }
            }
            Ok(nodes)
        }
    }

    deserializer.deserialize_map(Nodes)
}

/// The path of a capture: the deepest directory that contains every captured file, as given,
/// holding the directories and files in it on the way to each.
struct CapturePath<'a> {
    path: PathBuf,
    dir: Dir<'a>,
}

/// The directories and files in a directory on the way to a captured file, in the order of the
/// captured files.
#[derive(Default)]
struct Dir<'a> {
    entries: Vec<Entry<'a>>,
}

enum Entry<'a> {
    Dir(Name, Dir<'a>),
    File(Name, &'a FileSurface),
}

impl<'a> CapturePath<'a> {
    /// The deepest directory containing every one of `files`, holding each of them by the names
    /// of the directories below it on the way to the file and the file's own.
    fn enclosing(files: &'a [FileSurface]) -> Result<Self> {
        let components = |file: &'a FileSurface| Vec::from_iter(file.path().components());
        let common = files
            .iter()
            .map(|file| {
                let mut dirs = components(file);
                dirs.pop();
                dirs
            })
            .reduce(|mut common, dirs| {
                let shared = common.iter().zip(&dirs).take_while(|(a, b)| a == b).count();
                common.truncate(shared);
                common
            })
            .unwrap_or_default();
        let path = PathBuf::from_iter(&common);
        let mut dir = Dir::default();
        for file in files {
            let names = components(file)[common.len()..]
                .iter()
                .map(|&component| Name::try_from(component))
                .collect::<Result<Vec<_>, _>>()
                .with_context(|| {
                    format!(
                        "capturing {} below {:?}",
                        file.path().display(),
                        path.display().to_string()
                    )
                })?;
            let Some((name, dirs)) = names.split_last() else {
                bail!(
                    "capturing {}: the path names no file",
                    file.path().display()
                );
            };
            dir.subdir(dirs)
                .entries
                .push(Entry::File(name.clone(), file));
        }
        Ok(Self { path, dir })
    }

    /// Adds the nodes of this path and of everything in it to `json`, numbering each node
    /// before the nodes it encloses, and returns this path's id.
    fn number(&'a self, json: &mut Numbered<'a>) -> CaptureId {
        let id = json.next();
        let children = self.dir.number(json);
        json.nodes.insert(
            id,
            NodeJson {
                node: AnyNode::Path {
                    kind: "path",
                    path: &self.path,
                },
                children,
            },
        );
        id
    }
}

impl<'a> Dir<'a> {
    /// The directory reached from this one through the directories `names`, added where missing.
    fn subdir(&mut self, names: &[Name]) -> &mut Self {
        let Some((first, rest)) = names.split_first() else {
            return self;
        };
        let index = match self
            .entries
            .iter()
            .position(|entry| matches!(entry, Entry::Dir(name, _) if name == first))
        {
            Some(index) => index,
            None => {
                self.entries.push(Entry::Dir(first.clone(), Dir::default()));
                self.entries.len() - 1
            }
        };
        match &mut self.entries[index] {
            Entry::Dir(_, dir) => dir.subdir(rest),
            Entry::File(..) => unreachable!("the entry at index is a directory"),
        }
    }

    /// Adds the nodes of everything in this directory to `json`, numbering each node before the
    /// nodes it encloses, and returns the ids of the entries.
    fn number(&'a self, json: &mut Numbered<'a>) -> Vec<CaptureId> {
        self.entries
            .iter()
            .map(|entry| match entry {
                Entry::Dir(name, dir) => {
                    let id = json.next();
                    let children = dir.number(json);
                    json.nodes.insert(
                        id,
                        NodeJson {
                            node: AnyNode::Dir { kind: "dir", name },
                            children,
                        },
                    );
                    id
                }
                Entry::File(name, file) => json.file(name, file),
            })
            .collect()
    }
}

/// The nodes of a capture's JSON numbered so far.
#[derive(Default)]
struct Numbered<'a> {
    nodes: BTreeMap<CaptureId, NodeJson<'a>>,
    count: usize,
}

impl<'a> Numbered<'a> {
    fn next(&mut self) -> CaptureId {
        self.count += 1;
        CaptureId(self.count - 1)
    }

    /// Adds the nodes of `file`, named `name` in its directory, numbering the file's node before
    /// the nodes of the file in source order, and returns the file's id.
    fn file(&mut self, name: &'a Name, file: &'a FileSurface) -> CaptureId {
        let file_id = self.next();
        let nodes: Vec<(NodeId, Option<NodeId>, AnyNode)> = match file {
            FileSurface::Rust(file) => file
                .nodes()
                .iter()
                .map(|node| (node.id, node.parent, AnyNode::Rust(node)))
                .collect(),
            FileSurface::Python(file) => file
                .nodes()
                .iter()
                .map(|node| (node.id, node.parent, AnyNode::Python(node)))
                .collect(),
        };
        let ids = BTreeMap::from_iter(nodes.iter().map(|&(id, _, _)| (id, self.next())));
        // The ids of the nodes each node encloses, the file's under `None`.
        let mut children: BTreeMap<Option<NodeId>, Vec<CaptureId>> = BTreeMap::new();
        for &(id, parent, _) in &nodes {
            children.entry(parent).or_default().push(ids[&id]);
        }
        self.nodes.insert(
            file_id,
            NodeJson {
                node: AnyNode::File {
                    kind: "file",
                    language: file.language(),
                    name,
                },
                children: children.remove(&None).unwrap_or_default(),
            },
        );
        for (id, _, node) in nodes {
            self.nodes.insert(
                ids[&id],
                NodeJson {
                    node,
                    children: children.remove(&Some(id)).unwrap_or_default(),
                },
            );
        }
        file_id
    }
}

/// Numbers the nodes from the root down, each node before the nodes it encloses.
impl Serialize for CapturedSurface {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Json<'a> {
            root: CaptureId,
            nodes: BTreeMap<CaptureId, NodeJson<'a>>,
        }

        let path = CapturePath::enclosing(&self.files)
            .map_err(|error| <S::Error as ser::Error>::custom(format!("{error:#}")))?;
        let mut numbered = Numbered::default();
        let root = path.number(&mut numbered);
        Json {
            root,
            nodes: numbered.nodes,
        }
        .serialize(serializer)
    }
}

impl CapturedSurface {
    fn load(path: &Path) -> Result<Self> {
        let json =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&json).with_context(|| format!("parsing {}", path.display()))
    }

    /// The capture `json` holds, its files in the order the directories from the root list
    /// them, each at the root's path joined with the names of the directories on the way to it
    /// and its own. Every node is listed exactly once: the path's node as `root`, and every
    /// other node among the children of another.
    fn parse(json: &str) -> Result<Self> {
        let CaptureJson { root, nodes } = serde_json::from_str(json)?;
        let mut unplaced = Unplaced {
            nodes,
            placed: BTreeSet::new(),
        };
        let (path, children) = match unplaced.place(root)? {
            (Placed::Path(path), children) => (path.path, children),
            (Placed::Dir(_) | Placed::File(_) | Placed::Declaration(_), _) => {
                bail!("root {} is not a path node", root.0)
            }
        };
        let mut files = Vec::new();
        // Nodes the path or a directory lists, beside its path; the next is last.
        let mut pending = Vec::from_iter(children.into_iter().rev().map(|id| (id, path.clone())));
        while let Some((id, dir)) = pending.pop() {
            match unplaced.place(id)? {
                (Placed::Dir(child), children) => {
                    let path = dir.join(child.name);
                    pending.extend(children.into_iter().rev().map(|id| (id, path.clone())));
                }
                (Placed::File(file), children) => files.push(FileSurface::from_json(
                    dir.join(file.name),
                    file.language,
                    unplaced.place_file(children)?,
                )?),
                (Placed::Path(_), _) => bail!("path node {} is listed as a child", id.0),
                (Placed::Declaration(_), _) => {
                    bail!(
                        "declaration node {} is listed as a child of a path or dir node",
                        id.0
                    )
                }
            }
        }
        if let Some(id) = unplaced.nodes.keys().next() {
            bail!("node {} is not reachable from the root", id.0);
        }
        Ok(Self { files })
    }

    fn languages(&self) -> BTreeSet<Language> {
        self.files.iter().map(FileSurface::language).collect()
    }

    fn spec(
        &self,
        language: Language,
        rust: &rust::Includes,
    ) -> Result<(String, Vec<anyhow::Error>)> {
        let (declarations, errors) = split(
            self.files
                .iter()
                .filter(|file| file.language() == language && !file.is_empty())
                .filter_map(|file| {
                    let parts = BTreeSet::from_iter(file.parts(rust));
                    (!parts.is_empty()).then(|| file.declarations(&parts))
                }),
        );
        Ok((Spec::SurfaceSpec { declarations }.render(language)?, errors))
    }
}

/// The nodes of a capture's JSON not yet reached from its root.
struct Unplaced {
    nodes: BTreeMap<CaptureId, serde_json::Value>,
    placed: BTreeSet<CaptureId>,
}

impl Unplaced {
    /// Takes node `id`, which the root or another node lists, beside the ids it lists.
    fn place(&mut self, id: CaptureId) -> Result<(Placed, Vec<CaptureId>)> {
        let Some(node) = self.nodes.remove(&id) else {
            if self.placed.contains(&id) {
                bail!(
                    "node {} is listed more than once among the root and children",
                    id.0
                );
            }
            bail!("node {} is listed but is not among the nodes", id.0);
        };
        self.placed.insert(id);
        let parsing = || format!("parsing node {}", id.0);
        let Children { children } = Children::deserialize(&node).with_context(parsing)?;
        let placed = match NodeKind::deserialize(&node).with_context(parsing)? {
            NodeKind::Path(path) => Placed::Path(path),
            NodeKind::Dir(dir) => Placed::Dir(dir),
            NodeKind::File(file) => Placed::File(file),
            NodeKind::Declaration => Placed::Declaration(node),
        };
        Ok((placed, children))
    }

    /// Places the nodes a file's node lists as `children` and the nodes they enclose, in
    /// source order, each numbered by its position in the file.
    fn place_file(&mut self, children: Vec<CaptureId>) -> Result<Vec<PlacedNode>> {
        let mut file = Vec::new();
        // Nodes to place, beside the node enclosing each within the file; the next is last.
        let mut pending = Vec::from_iter(children.into_iter().rev().map(|id| (id, None)));
        while let Some((capture_id, parent)) = pending.pop() {
            let (json, children) = match self.place(capture_id)? {
                (Placed::Declaration(json), children) => (json, children),
                (Placed::Path(_), _) => bail!("path node {} is listed as a child", capture_id.0),
                (Placed::Dir(_), _) => {
                    bail!("dir node {} is listed as a child of a file", capture_id.0)
                }
                (Placed::File(_), _) => {
                    bail!("file node {} is listed as a child of a file", capture_id.0)
                }
            };
            let id = NodeId(file.len());
            file.push(PlacedNode {
                capture_id,
                id,
                parent,
                json,
            });
            pending.extend(children.into_iter().rev().map(|child| (child, Some(id))));
        }
        Ok(file)
    }
}

/// A node reached from the root: the path's, a directory's, a file's, or a declaration's JSON,
/// read once its file is known.
enum Placed {
    Path(PathJson),
    Dir(DirJson),
    File(FileJson),
    Declaration(serde_json::Value),
}

struct PlacedNode {
    capture_id: CaptureId,
    id: NodeId,
    parent: Option<NodeId>,
    json: serde_json::Value,
}

enum FileChange {
    Added(FileSurface),
    Deleted(FileSurface),
    Kept {
        base: FileSurface,
        head: FileSurface,
    },
}

impl FileChange {
    fn pair(
        mut base: BTreeMap<PathBuf, Result<FileSurface>>,
        head: BTreeMap<PathBuf, Result<FileSurface>>,
    ) -> (Vec<Self>, Vec<anyhow::Error>) {
        let mut changes = Vec::new();
        let mut errors = Vec::new();
        for (path, head) in head {
            match (base.remove(&path), head) {
                (None, Ok(head)) => changes.push(Self::Added(head)),
                (Some(Ok(base)), Ok(head)) => changes.push(Self::Kept { base, head }),
                (None | Some(Ok(_)), Err(error)) | (Some(Err(error)), Ok(_)) => errors.push(error),
                (Some(Err(base_error)), Err(head_error)) => errors.extend([base_error, head_error]),
            }
        }
        for surface in base.into_values() {
            match surface {
                Ok(base) => changes.push(Self::Deleted(base)),
                Err(error) => errors.push(error),
            }
        }
        changes.sort_by(|a, b| a.path().cmp(b.path()));
        (changes, errors)
    }

    fn spec(
        changes: &[Self],
        language: Language,
        rust: &rust::Includes,
    ) -> Result<(String, Vec<anyhow::Error>)> {
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut errors = Vec::new();
        for change in changes
            .iter()
            .filter(|change| change.language() == language)
        {
            match change.declarations(rust) {
                Ok((added, deleted)) => {
                    additions.extend(added);
                    deletions.extend(deleted);
                }
                Err(error) => errors.push(error),
            }
        }
        let spec = Spec::Change {
            additions,
            deletions,
        };
        Ok((spec.render(language)?, errors))
    }

    fn declarations(
        &self,
        rust: &rust::Includes,
    ) -> Result<(Option<FileDeclarations>, Option<FileDeclarations>)> {
        Ok(match self {
            Self::Added(head) => (head.declarations_lacking(None, rust)?, None),
            Self::Deleted(base) => (None, base.declarations_lacking(None, rust)?),
            Self::Kept { base, head } => (
                head.declarations_lacking(Some(base), rust)?,
                base.declarations_lacking(Some(head), rust)?,
            ),
        })
    }

    fn language(&self) -> Language {
        self.surface().language()
    }

    fn path(&self) -> &Path {
        self.surface().path()
    }

    fn surface(&self) -> &FileSurface {
        match self {
            Self::Added(head) | Self::Kept { head, .. } => head,
            Self::Deleted(base) => base,
        }
    }
}

enum Spec {
    Change {
        additions: Vec<FileDeclarations>,
        deletions: Vec<FileDeclarations>,
    },
    SurfaceSpec {
        declarations: Vec<FileDeclarations>,
    },
}

impl Spec {
    fn render(self, language: Language) -> Result<String> {
        let mut env = Environment::new();
        env.set_syntax(
            SyntaxConfig::builder()
                .keep_trailing_newline(true)
                .build()?,
        );
        env.add_template("common.md.j2", COMMON)?;
        env.add_template("language.md.j2", language.template())?;
        let values =
            |files: Vec<FileDeclarations>| Vec::from_iter(files.into_iter().map(Value::from));
        let (template, declarations) = match self {
            Self::Change {
                additions,
                deletions,
            } => (
                CHANGE,
                context! { additions => values(additions), deletions => values(deletions) },
            ),
            Self::SurfaceSpec { declarations } => (
                SURFACE_SPEC,
                context! { declarations => values(declarations) },
            ),
        };
        Ok(env.template_from_str(template)?.render(declarations)?)
    }
}

struct FileDeclarations {
    path: String,
    code: String,
}

impl From<FileDeclarations> for Value {
    fn from(file: FileDeclarations) -> Self {
        context! { path => file.path, code => file.code }
    }
}

enum FileSurface {
    Rust(rust::FileSurface),
    Python(python::FileSurface),
}

impl FileSurface {
    fn read(path: PathBuf, language: Language) -> Result<Self> {
        let source =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(path, language, &source)
    }

    fn parse(path: PathBuf, language: Language, source: &str) -> Result<Self> {
        match language {
            Language::Rust => rust::FileSurface::parse(path, source).map(Self::Rust),
            Language::Python => python::FileSurface::parse(path, source).map(Self::Python),
        }
    }

    /// The surface of the `language` file at `path`, holding `nodes` in source order.
    fn from_json(path: PathBuf, language: Language, nodes: Vec<PlacedNode>) -> Result<Self> {
        fn typed<N: DeserializeOwned>(
            nodes: Vec<PlacedNode>,
            links: fn(&mut N) -> (&mut NodeId, &mut Option<NodeId>),
        ) -> Result<Vec<N>> {
            nodes
                .into_iter()
                .map(|placed| {
                    let mut node = N::deserialize(placed.json)
                        .with_context(|| format!("parsing node {}", placed.capture_id.0))?;
                    let (id, parent) = links(&mut node);
                    *id = placed.id;
                    *parent = placed.parent;
                    Ok(node)
                })
                .collect()
        }

        Ok(match language {
            Language::Rust => Self::Rust(rust::FileSurface::new(
                path,
                typed(nodes, |node: &mut rust::Node| {
                    (&mut node.id, &mut node.parent)
                })?,
            )),
            Language::Python => Self::Python(python::FileSurface::new(
                path,
                typed(nodes, |node: &mut python::Node| {
                    (&mut node.id, &mut node.parent)
                })?,
            )),
        })
    }

    fn language(&self) -> Language {
        match self {
            Self::Rust(_) => Language::Rust,
            Self::Python(_) => Language::Python,
        }
    }

    fn path(&self) -> &Path {
        match self {
            Self::Rust(file) => file.path(),
            Self::Python(file) => file.path(),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Rust(file) => file.is_empty(),
            Self::Python(file) => file.is_empty(),
        }
    }

    /// The nodes a change adds or deletes as a whole, among those `rust` keeps when it is a
    /// Rust file.
    fn parts(&self, rust: &rust::Includes) -> Vec<NodeId> {
        match self {
            Self::Rust(file) => file.parts(rust),
            Self::Python(file) => file.parts(),
        }
    }

    fn declarations(&self, parts: &BTreeSet<NodeId>) -> Result<FileDeclarations> {
        let code = match self {
            Self::Rust(file) => Ok(file.declarations(parts)),
            Self::Python(file) => file.declarations(parts),
        }
        .with_context(|| format!("rendering {}", self.path().display()))?;
        Ok(FileDeclarations {
            path: self.path().display().to_string(),
            code,
        })
    }

    /// The declarations of the parts `other` lacks, each part matching one equal to it in
    /// `other` by text; `None` when `other` has them all.
    fn declarations_lacking(
        &self,
        other: Option<&Self>,
        rust: &rust::Includes,
    ) -> Result<Option<FileDeclarations>> {
        let mut unmatched: BTreeMap<String, usize> = BTreeMap::new();
        if let Some(other) = other {
            for part in other.parts(rust) {
                *unmatched.entry(other.text(part)?).or_default() += 1;
            }
        }
        let mut lacking = BTreeSet::new();
        for part in self.parts(rust) {
            match unmatched.get_mut(&self.text(part)?) {
                Some(count) if *count > 0 => *count -= 1,
                Some(_) | None => {
                    lacking.insert(part);
                }
            }
        }
        if lacking.is_empty() {
            return Ok(None);
        }
        self.declarations(&lacking).map(Some)
    }

    /// `part` as text, inside the nodes enclosing it, which equals the text of an equal part.
    fn text(&self, part: NodeId) -> Result<String> {
        let parts = BTreeSet::from([part]);
        Ok(match self {
            Self::Rust(file) => file.declarations(&parts),
            Self::Python(file) => file.text(&parts),
        })
    }
}

/// A node's position in its file, in source order, by which its children name it as their
/// parent.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct NodeId(usize);

#[derive(Serialize, Deserialize)]
struct Span {
    start: Position,
    end: Position,
}

/// A 1-based line and a 0-based column in characters.
#[derive(Serialize, Deserialize)]
struct Position {
    line: usize,
    column: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file `$file` of fixture `$case` in `$dir`.
    macro_rules! fixture_file {
        ($dir:literal, $case:literal, $file:literal) => {
            include_str!(concat!(
                "surface_spec/testdata/",
                $dir,
                "/",
                $case,
                "/",
                $file
            ))
        };
    }

    macro_rules! fixture {
        ($language:ident, $dir:literal, $extension:literal, $case:literal) => {
            Fixture {
                language: Language::$language,
                path: concat!($case, ".", $extension),
                source: fixture_file!($dir, $case, "source"),
                capture: fixture_file!($dir, $case, "capture"),
                additions: fixture_file!($dir, $case, "additions"),
                deletions: fixture_file!($dir, $case, "deletions"),
                surface_spec: fixture_file!($dir, $case, "surface-spec"),
            }
        };
    }

    macro_rules! rust_fixture {
        ($case:literal) => {
            fixture!(Rust, "rust", "rs", $case)
        };
    }

    macro_rules! python_fixture {
        ($case:literal) => {
            fixture!(Python, "python", "py", $case)
        };
    }

    macro_rules! error_fixture {
        ($language:ident, $dir:literal, $extension:literal, $case:literal) => {
            ErrorFixture {
                language: Language::$language,
                path: concat!($case, ".", $extension),
                source: fixture_file!($dir, $case, "source"),
                error: fixture_file!($dir, $case, "error"),
            }
        };
    }

    macro_rules! rust_error_fixture {
        ($case:literal) => {
            error_fixture!(Rust, "rust", "rs", $case)
        };
    }

    macro_rules! python_error_fixture {
        ($case:literal) => {
            error_fixture!(Python, "python", "py", $case)
        };
    }

    struct Fixture {
        language: Language,
        path: &'static str,
        source: &'static str,
        capture: &'static str,
        additions: &'static str,
        deletions: &'static str,
        surface_spec: &'static str,
    }

    impl Fixture {
        fn check(&self) {
            let file = FileSurface::parse(PathBuf::from(self.path), self.language, self.source)
                .expect("capturable fixture");
            let captured =
                serde_json::to_value(CapturedSurface { files: vec![file] }).expect("JSON surface");
            let expected: serde_json::Value =
                serde_json::from_str(self.capture).expect("JSON fixture");
            assert_eq!(captured, expected);

            let surface = || -> CapturedSurface {
                CapturedSurface::parse(self.capture).expect("captured surface")
            };
            for (change, expected) in [
                (
                    FileChange::Added as fn(FileSurface) -> FileChange,
                    self.additions,
                ),
                (FileChange::Deleted, self.deletions),
            ] {
                let changes = Vec::from_iter(surface().files.into_iter().map(change));
                let (spec, errors) =
                    FileChange::spec(&changes, self.language, &rust::Includes::default())
                        .expect("rendered spec");
                assert!(errors.is_empty());
                assert_eq!(spec, expected);
            }
            let (surface_spec, errors) = surface()
                .spec(self.language, &rust::Includes::default())
                .expect("rendered spec");
            assert!(errors.is_empty());
            assert_eq!(surface_spec, self.surface_spec);
        }
    }

    macro_rules! change_fixture {
        ($language:ident, $dir:literal, $extension:literal, $case:literal) => {
            ChangeFixture {
                language: Language::$language,
                path: concat!($case, ".", $extension),
                base: fixture_file!($dir, $case, "base"),
                head: fixture_file!($dir, $case, "head"),
                change: fixture_file!($dir, $case, "change"),
            }
        };
    }

    /// A source before and after a change, and the change surface spec between them.
    struct ChangeFixture {
        language: Language,
        path: &'static str,
        base: &'static str,
        head: &'static str,
        change: &'static str,
    }

    impl ChangeFixture {
        fn check(&self) {
            let parse = |source| {
                FileSurface::parse(PathBuf::from(self.path), self.language, source)
                    .expect("capturable fixture")
            };
            let change = FileChange::Kept {
                base: parse(self.base),
                head: parse(self.head),
            };
            let (spec, errors) =
                FileChange::spec(&[change], self.language, &rust::Includes::default())
                    .expect("rendered spec");
            assert!(errors.is_empty());
            assert_eq!(spec, self.change);
        }
    }

    /// A source surface-spec cannot capture, and the error it fails with.
    struct ErrorFixture {
        language: Language,
        path: &'static str,
        source: &'static str,
        error: &'static str,
    }

    impl ErrorFixture {
        fn check(&self) {
            let error = FileSurface::parse(PathBuf::from(self.path), self.language, self.source)
                .err()
                .expect("uncapturable fixture");
            assert_eq!(format!("{error:#}\n"), self.error);
        }
    }

    #[test]
    fn structs() {
        rust_fixture!("structs").check();
    }

    #[test]
    fn enums() {
        rust_fixture!("enums").check();
    }

    #[test]
    fn traits() {
        rust_fixture!("traits").check();
    }

    #[test]
    fn inherent_impls() {
        rust_fixture!("inherent_impls").check();
    }

    #[test]
    fn trait_impls() {
        rust_fixture!("trait_impls").check();
    }

    #[test]
    fn fn_qualifiers() {
        rust_fixture!("fn_qualifiers").check();
    }

    #[test]
    fn bodiless_fns() {
        rust_fixture!("bodiless_fns").check();
    }

    #[test]
    fn nested() {
        rust_fixture!("nested").check();
    }

    #[test]
    fn extern_blocks() {
        rust_fixture!("extern_blocks").check();
    }

    #[test]
    fn ignored() {
        rust_fixture!("ignored").check();
    }

    #[test]
    fn empty() {
        rust_fixture!("empty").check();
    }

    #[test]
    fn changes() {
        change_fixture!(Rust, "rust", "rs", "changes").check();
    }

    #[test]
    fn syntax_error() {
        rust_error_fixture!("syntax_error").check();
    }

    #[test]
    fn safe_foreign_fn() {
        rust_error_fixture!("safe_foreign_fn").check();
    }

    #[test]
    fn precise_capture() {
        rust_error_fixture!("precise_capture").check();
    }

    #[test]
    fn python_functions() {
        python_fixture!("functions").check();
    }

    #[test]
    fn python_classes() {
        python_fixture!("classes").check();
    }

    #[test]
    fn python_nested() {
        python_fixture!("nested").check();
    }

    #[test]
    fn python_expressions() {
        python_fixture!("expressions").check();
    }

    #[test]
    fn python_empty() {
        python_fixture!("empty").check();
    }

    #[test]
    fn python_changes() {
        change_fixture!(Python, "python", "py", "changes").check();
    }

    #[test]
    fn python_syntax_error() {
        python_error_fixture!("syntax_error").check();
    }
    fn parse(path: &str, language: Language, source: &str) -> FileSurface {
        FileSurface::parse(PathBuf::from(path), language, source).expect("capturable source")
    }

    /// The `(id, parent)` of each node of each file in `surface`.
    fn links(surface: &CapturedSurface) -> Vec<Vec<(usize, Option<usize>)>> {
        let link = |id: NodeId, parent: Option<NodeId>| (id.0, parent.map(|parent| parent.0));
        surface
            .files
            .iter()
            .map(|file| match file {
                FileSurface::Rust(file) => {
                    Vec::from_iter(file.nodes().iter().map(|node| link(node.id, node.parent)))
                }
                FileSurface::Python(file) => {
                    Vec::from_iter(file.nodes().iter().map(|node| link(node.id, node.parent)))
                }
            })
            .collect()
    }

    #[test]
    fn capture_ids_are_unique_across_files() {
        let surface = CapturedSurface {
            files: vec![
                parse("a.rs", Language::Rust, "struct A { x: u8 }"),
                parse("b.py", Language::Python, "class B:\n    y: int\n"),
                parse("c.rs", Language::Rust, "enum C { D(u8) }"),
            ],
        };
        let json = serde_json::to_value(&surface).expect("JSON surface");
        assert_eq!(json["root"], serde_json::json!(0));
        let nodes = json["nodes"].as_object().expect("nodes by id");
        let mut kinds = Vec::from_iter(nodes.iter().map(|(id, node)| {
            (
                id.as_str(),
                node["kind"].as_str(),
                node.get("children").cloned(),
            )
        }));
        kinds.sort_by_key(|&(id, _, _)| id.parse::<usize>().expect("numeric id"));
        let children = |ids: &[usize]| Some(serde_json::json!(ids));
        assert_eq!(
            kinds,
            [
                ("0", Some("path"), children(&[1, 4, 7])),
                ("1", Some("file"), children(&[2])),
                ("2", Some("struct"), children(&[3])),
                ("3", Some("field"), None),
                ("4", Some("file"), children(&[5])),
                ("5", Some("class"), children(&[6])),
                ("6", Some("attribute"), None),
                ("7", Some("file"), children(&[8])),
                ("8", Some("enum"), children(&[9])),
                ("9", Some("variant"), children(&[10])),
                ("10", Some("field"), None),
            ],
        );
        assert_eq!(
            nodes["0"],
            serde_json::json!({"kind": "path", "path": "", "children": [1, 4, 7]}),
        );
        assert_eq!(
            nodes["4"],
            serde_json::json!({"kind": "file", "language": "python", "name": "b.py", "children": [5]}),
        );
        assert!(
            nodes
                .values()
                .all(|node| node.get("id").is_none() && node.get("parent").is_none())
        );

        let parsed = CapturedSurface::parse(&json.to_string()).expect("capture");
        assert_eq!(
            links(&parsed),
            [
                vec![(0, None), (1, Some(0))],
                vec![(0, None), (1, Some(0))],
                vec![(0, None), (1, Some(0)), (2, Some(1))],
            ],
        );
        assert_eq!(serde_json::to_value(&parsed).expect("JSON surface"), json);
    }

    #[test]
    fn capture_dirs_lead_to_each_file() {
        let surface = CapturedSurface {
            files: vec![
                parse("/src/p/a/x.rs", Language::Rust, ""),
                parse("/src/p/b.py", Language::Python, ""),
                parse("/src/p/a/c/y.rs", Language::Rust, ""),
            ],
        };
        let json = serde_json::to_value(&surface).expect("JSON surface");
        assert_eq!(
            json,
            serde_json::json!({
                "root": 0,
                "nodes": {
                    "0": {"kind": "path", "path": "/src/p", "children": [1, 5]},
                    "1": {"kind": "dir", "name": "a", "children": [2, 3]},
                    "2": {"kind": "file", "language": "rust", "name": "x.rs"},
                    "3": {"kind": "dir", "name": "c", "children": [4]},
                    "4": {"kind": "file", "language": "rust", "name": "y.rs"},
                    "5": {"kind": "file", "language": "python", "name": "b.py"},
                },
            }),
        );
        let parsed = CapturedSurface::parse(&json.to_string()).expect("capture");
        assert_eq!(
            Vec::from_iter(parsed.files.iter().map(FileSurface::path)),
            [
                Path::new("/src/p/a/x.rs"),
                Path::new("/src/p/a/c/y.rs"),
                Path::new("/src/p/b.py"),
            ],
        );
    }

    #[test]
    fn capture_files_need_names_below_the_path() {
        for paths in [
            ["/a/x.rs", "y.rs"],
            ["../x.rs", "y.rs"],
            ["a/../x.rs", "a/y.rs"],
        ] {
            let surface = CapturedSurface {
                files: Vec::from_iter(paths.map(|path| parse(path, Language::Rust, ""))),
            };
            let error = serde_json::to_value(&surface).expect_err("uncapturable paths");
            assert!(
                error.to_string().ends_with("is not a single path segment"),
                "{error}"
            );
        }
    }

    #[test]
    fn names_are_one_segment() {
        for name in ["a", "a.rs", ".a", "a..b"] {
            assert!(Name::try_from(name.to_owned()).is_ok(), "{name}");
        }
        for name in ["", ".", "..", "/", "a/", "a/.", "./a", "a/b", "/a"] {
            assert!(Name::try_from(name.to_owned()).is_err(), "{name:?}");
        }
    }

    #[test]
    fn capture_root_is_the_only_node_without_files() {
        let json = serde_json::to_value(CapturedSurface { files: Vec::new() }).expect("JSON");
        assert_eq!(
            json,
            serde_json::json!({"root": 0, "nodes": {"0": {"kind": "path", "path": ""}}}),
        );
        let parsed = CapturedSurface::parse(&json.to_string()).expect("capture");
        assert!(parsed.files.is_empty());
    }

    /// A capture of file `a.py` holding class `A` with attribute `y`: the path, file, class, and
    /// attribute nodes keyed by `ids`, their children set by `children`, beside
    /// `extra` nodes, with `root` as the root.
    fn capture(
        ids: [usize; 4],
        root: usize,
        children: &[(usize, &[usize])],
        extra: &[(usize, serde_json::Value)],
    ) -> String {
        let surface = CapturedSurface {
            files: vec![parse("a.py", Language::Python, "class A:\n    y: int\n")],
        };
        let mut json = serde_json::to_value(&surface).expect("JSON surface");
        let mut nodes = serde_json::Map::new();
        for (index, id) in ids.into_iter().enumerate() {
            let mut node = json["nodes"][index.to_string()].take();
            node.as_object_mut().expect("node").remove("children");
            nodes.insert(id.to_string(), node);
        }
        for (id, node) in extra {
            nodes.insert(id.to_string(), node.clone());
        }
        for (id, ids) in children {
            nodes[&id.to_string()]["children"] = serde_json::json!(ids);
        }
        json["nodes"] = nodes.into();
        json["root"] = serde_json::json!(root);
        json.to_string()
    }

    #[test]
    fn capture_ids_renumber_loaded_files() {
        let parsed = CapturedSurface::parse(&capture(
            [5, 4, 9, 7],
            5,
            &[(5, &[4]), (4, &[9]), (9, &[7])],
            &[],
        ))
        .expect("capture");
        assert_eq!(links(&parsed), [vec![(0, None), (1, Some(0))]]);
        let canonical = CapturedSurface::parse(&capture(
            [0, 1, 2, 3],
            0,
            &[(0, &[1]), (1, &[2]), (2, &[3])],
            &[],
        ))
        .expect("capture");
        assert_eq!(
            serde_json::to_value(&parsed).expect("JSON surface"),
            serde_json::to_value(&canonical).expect("JSON surface"),
        );
    }

    #[test]
    fn capture_loads_deep_nesting() {
        const DEPTH: usize = 1000;
        let base: serde_json::Value =
            serde_json::from_str(&capture([0, 1, 2, 3], 0, &[(0, &[1]), (1, &[2])], &[]))
                .expect("JSON");
        let mut nodes = serde_json::Map::new();
        nodes.insert("0".into(), base["nodes"]["0"].clone());
        nodes.insert("1".into(), base["nodes"]["1"].clone());
        for id in 2..DEPTH + 2 {
            let mut class = base["nodes"]["2"].clone();
            if id < DEPTH + 1 {
                class["children"] = serde_json::json!([id + 1]);
            }
            nodes.insert(id.to_string(), class);
        }
        let json = serde_json::json!({"root": 0, "nodes": nodes});
        let parsed = CapturedSurface::parse(&json.to_string()).expect("capture");
        assert_eq!(links(&parsed)[0].len(), DEPTH);
        assert_eq!(links(&parsed)[0][DEPTH - 1], (DEPTH - 1, Some(DEPTH - 2)));
    }

    #[test]
    fn capture_ids_reject_misplaced_nodes() {
        let file = serde_json::json!({"kind": "file", "language": "python", "name": "b.py"});
        let dir = serde_json::json!({"kind": "dir", "name": "d"});
        let path = serde_json::json!({"kind": "path", "path": "p"});
        let linked = |class: &[usize], extra: &[(usize, serde_json::Value)]| {
            capture([0, 1, 2, 3], 0, &[(0, &[1]), (1, &[2]), (2, class)], extra)
        };
        for (json, error) in [
            (
                linked(&[3], &[]).replace(r#""3":"#, r#""2":"#),
                "node 2 appears more than once at line 1 column ",
            ),
            (
                linked(&[3, 3], &[]),
                "node 3 is listed more than once among the root and children",
            ),
            (
                capture(
                    [0, 1, 2, 3],
                    0,
                    &[(0, &[1]), (1, &[2]), (2, &[3]), (3, &[2])],
                    &[],
                ),
                "node 2 is listed more than once among the root and children",
            ),
            (
                linked(&[3, 5], &[]),
                "node 5 is listed but is not among the nodes",
            ),
            (
                capture([0, 1, 2, 3], 1, &[(1, &[2]), (2, &[3])], &[]),
                "root 1 is not a path node",
            ),
            (
                linked(&[3, 4], &[(4, file)]),
                "file node 4 is listed as a child of a file",
            ),
            (
                linked(&[3, 4], &[(4, dir)]),
                "dir node 4 is listed as a child of a file",
            ),
            (
                linked(&[3, 4], &[(4, path.clone())]),
                "path node 4 is listed as a child",
            ),
            (
                capture(
                    [0, 1, 2, 3],
                    0,
                    &[(0, &[1, 4]), (1, &[2]), (2, &[3])],
                    &[(4, path)],
                ),
                "path node 4 is listed as a child",
            ),
            (
                capture([0, 1, 2, 3], 0, &[(0, &[1, 2]), (2, &[3])], &[]),
                "declaration node 2 is listed as a child of a path or dir node",
            ),
            (
                capture(
                    [0, 1, 2, 3],
                    0,
                    &[(0, &[4]), (4, &[1]), (1, &[2]), (2, &[3])],
                    &[(4, serde_json::json!({"kind": "dir", "name": "a/b"}))],
                ),
                "parsing node 4",
            ),
            (linked(&[], &[]), "node 3 is not reachable from the root"),
        ] {
            let parsed = CapturedSurface::parse(&json)
                .err()
                .expect("invalid capture");
            assert!(
                parsed.to_string().starts_with(error),
                "{parsed} does not start with {error}"
            );
        }
    }
}
