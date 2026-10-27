mod python;
mod rust;

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::slice;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use clap::{Args as ClapArgs, ValueEnum};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::{Walk, WalkBuilder};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, Value, context};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
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

fn unwind<T>(f: impl FnOnce() -> T) -> Result<T> {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(hook);
    result.map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        anyhow!("panicked: {message}")
    })
}

#[derive(Serialize, Deserialize)]
struct CapturedSurface {
    files: Vec<FileSurface>,
}

impl CapturedSurface {
    fn load(path: &Path) -> Result<Self> {
        let json =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&json).with_context(|| format!("parsing {}", path.display()))
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Record(RecordId),
    /// A class attribute, by its index among the class's attributes.
    Attribute(RecordId, usize),
}

impl Part {
    fn record(self) -> RecordId {
        match self {
            Self::Record(id) | Self::Attribute(id, _) => id,
        }
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

#[derive(Serialize, Deserialize)]
#[serde(tag = "language", rename_all = "lowercase")]
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
        let name = path.display().to_string();
        unwind(|| {
            Ok(match language {
                Language::Rust => Self::Rust(rust::FileSurface::parse(path, source)?),
                Language::Python => Self::Python(python::FileSurface::parse(path, source)?),
            })
        })
        .with_context(|| format!("parsing {name}"))?
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

    /// The parts of the file, among the records `rust` keeps when it is a Rust file.
    fn parts(&self, rust: &rust::Includes) -> Vec<Part> {
        match self {
            Self::Rust(file) => file.parts(rust),
            Self::Python(file) => file.parts(),
        }
    }

    fn declarations(&self, parts: &BTreeSet<Part>) -> Result<FileDeclarations> {
        let code = unwind(|| match self {
            Self::Rust(file) => Ok(file.declarations(parts)),
            Self::Python(file) => file.declarations(parts),
        })
        .and_then(|declarations| declarations)
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

    /// `part` as text, inside the records enclosing it, which equals the text of an equal part.
    fn text(&self, part: Part) -> Result<String> {
        let parts = BTreeSet::from([part]);
        unwind(|| match self {
            Self::Rust(file) => file.declarations(&parts),
            Self::Python(file) => file.text(&parts),
        })
        .with_context(|| format!("rendering {}", self.path().display()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct RecordId(usize);

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
                serde_json::from_str(self.capture).expect("captured surface")
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
}
