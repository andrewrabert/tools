mod python;
mod rust;

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::slice;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use clap::{ArgGroup, Args as ClapArgs, Subcommand, ValueEnum};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::{Walk, WalkBuilder};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, Value, context};
use serde::{Deserialize, Serialize};

use crate::atomic_write;
use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(
    about = "Capture the declarations in Rust and Python source files and render them as a surface spec"
)]
pub struct SurfaceSpec {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the declarations in Rust and Python source files as JSON syntax trees
    Capture(Capture),
    /// Print a captured surface as the additions or deletions of a surface spec
    Render(Render),
}

#[derive(ClapArgs)]
struct Capture {
    /// A `.rs`, `.py`, or `.pyi` file, or a directory to search for them
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    filter: Filter,
    /// Write the JSON to this file instead of stdout
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

#[derive(ClapArgs)]
#[command(group(ArgGroup::new("section").required(true).args(["additions", "deletions"])))]
struct Render {
    /// A `.json` file printed by `capture`, or a source file or directory to capture
    #[arg(value_name = "PATH", required = true)]
    inputs: Vec<Input>,
    #[command(flatten)]
    filter: Filter,
    /// List the declarations as additions
    #[arg(short, long)]
    additions: bool,
    /// List the declarations as deletions
    #[arg(short, long)]
    deletions: bool,
    /// Render the spec for this language's files [default: every language captured]
    #[arg(short, long, value_enum)]
    language: Option<Language>,
    /// Write each language's spec to `<language>.md` in this directory instead of stdout;
    /// required to render more than one language
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,
}

impl Render {
    fn section(&self) -> Section {
        if self.additions {
            Section::Additions
        } else {
            Section::Deletions
        }
    }
}

#[derive(Clone)]
enum Input {
    Capture(PathBuf),
    Sources(PathBuf),
}

impl FromStr for Input {
    type Err = Infallible;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(path);
        Ok(match path.extension() {
            Some(extension) if extension == "json" => Self::Capture(path),
            _ => Self::Sources(path),
        })
    }
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

#[derive(Clone, Copy)]
enum Section {
    Additions,
    Deletions,
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

    fn spec(self) -> (&'static str, &'static str) {
        match self {
            Self::Rust => ("rust.md.j2", rust::SPEC),
            Self::Python => ("python.md.j2", python::SPEC),
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
        let result = match self.command {
            Command::Capture(args) => capture(args),
            Command::Render(args) => render(args),
        };
        match result {
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

fn capture(args: Capture) -> Result<(), Errors> {
    let selection = args.filter.select()?;
    let (files, mut errors) = split(read_sources(&args.paths, &selection));
    if let Err(error) = write_capture(&CapturedSurface { files }, args.output.as_deref()) {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Errors(errors))
    }
}

fn write_capture(surface: &CapturedSurface, output: Option<&Path>) -> Result<()> {
    let json = serde_json::to_string(surface)?;
    match output {
        Some(path) => write(path, &format!("{json}\n"))?,
        None => writeln!(io::stdout(), "{json}").context("writing to stdout")?,
    }
    Ok(())
}

fn render(args: Render) -> Result<(), Errors> {
    let selection = args.filter.select()?;
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for input in &args.inputs {
        match input {
            Input::Capture(path) => match CapturedSurface::load(path) {
                Ok(surface) => files.extend(
                    surface
                        .files
                        .into_iter()
                        .filter(|file| selection.keeps(file.path())),
                ),
                Err(error) => errors.push(error),
            },
            Input::Sources(path) => {
                let (sources, source_errors) =
                    split(read_sources(slice::from_ref(path), &selection));
                files.extend(sources);
                errors.extend(source_errors);
            }
        }
    }
    let surface = CapturedSurface { files };
    match write_specs(
        &surface,
        args.language,
        args.section(),
        args.output.as_deref(),
    ) {
        Ok(spec_errors) | Err(Errors(spec_errors)) => errors.extend(spec_errors),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Errors(errors))
    }
}

/// `Ok` holds the errors of files left out of the written specs; `Err` holds
/// those followed by the error that stopped the write.
fn write_specs(
    surface: &CapturedSurface,
    language: Option<Language>,
    section: Section,
    output: Option<&Path>,
) -> Result<Vec<anyhow::Error>, Errors> {
    let languages = match language {
        Some(language) => BTreeSet::from([language]),
        None => surface.languages(),
    };
    match (output, Vec::from_iter(languages).as_slice()) {
        (_, []) => {
            Err(anyhow!("no files to render; pass --language to render an empty spec").into())
        }
        (None, [language]) => {
            let (spec, errors) = surface.spec(*language, section)?;
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
                let (spec, spec_errors) = match surface.spec(language, section) {
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

    fn spec(&self, language: Language, section: Section) -> Result<(String, Vec<anyhow::Error>)> {
        let (files, errors): (Vec<Value>, _) = split(
            self.files
                .iter()
                .filter(|file| file.language() == language && !file.is_empty())
                .map(|file| {
                    let code = file.declarations()?;
                    Ok(context! {
                        path => file.path().display().to_string(),
                        code,
                    })
                }),
        );
        let (additions, deletions) = match section {
            Section::Additions => (files, Vec::new()),
            Section::Deletions => (Vec::new(), files),
        };
        let (name, template) = language.spec();
        let mut env = Environment::new();
        env.set_syntax(
            SyntaxConfig::builder()
                .keep_trailing_newline(true)
                .build()?,
        );
        env.add_template(name, template)?;
        let spec = env
            .get_template(name)?
            .render(context! { additions, deletions })?;
        Ok((spec, errors))
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

    fn declarations(&self) -> Result<String> {
        unwind(|| match self {
            Self::Rust(file) => Ok(file.declarations()),
            Self::Python(file) => file.declarations(),
        })
        .and_then(|declarations| declarations)
        .with_context(|| format!("rendering {}", self.path().display()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    macro_rules! fixture {
        ($language:ident, $dir:literal, $extension:literal, $case:literal) => {
            Fixture {
                language: Language::$language,
                path: concat!($case, ".", $extension),
                source: include_str!(concat!(
                    "surface_spec/testdata/",
                    $dir,
                    "/",
                    $case,
                    ".",
                    $extension
                )),
                capture: include_str!(concat!("surface_spec/testdata/", $dir, "/", $case, ".json")),
                additions: include_str!(concat!(
                    "surface_spec/testdata/",
                    $dir,
                    "/",
                    $case,
                    ".additions.md"
                )),
                deletions: include_str!(concat!(
                    "surface_spec/testdata/",
                    $dir,
                    "/",
                    $case,
                    ".deletions.md"
                )),
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

    struct Fixture {
        language: Language,
        path: &'static str,
        source: &'static str,
        capture: &'static str,
        additions: &'static str,
        deletions: &'static str,
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

            let surface: CapturedSurface =
                serde_json::from_str(self.capture).expect("captured surface");
            let (additions, errors) = surface
                .spec(self.language, Section::Additions)
                .expect("rendered spec");
            assert!(errors.is_empty());
            assert_eq!(additions, self.additions);
            let (deletions, errors) = surface
                .spec(self.language, Section::Deletions)
                .expect("rendered spec");
            assert!(errors.is_empty());
            assert_eq!(deletions, self.deletions);
        }
    }

    fn error(language: Language, path: &str, source: &str) -> anyhow::Error {
        FileSurface::parse(PathBuf::from(path), language, source)
            .err()
            .expect("uncapturable source")
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
    fn syntax_error() {
        error(Language::Rust, "error.rs", "fn f( {}\n");
    }

    #[test]
    fn safe_foreign_fn() {
        let source = "unsafe extern \"C\" {\n    fn f(x: i32) -> i32;\n    pub safe fn g();\n}\n";
        assert_eq!(
            error(Language::Rust, "error.rs", source).to_string(),
            "parsing error.rs:3:5: surface-spec cannot capture this foreign item"
        );
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
    fn python_syntax_error() {
        let message = error(Language::Python, "error.py", "def f(:\n    pass\n").to_string();
        assert!(
            message.starts_with("parsing error.py:1:"),
            "unexpected error: {message}"
        );
    }
}
