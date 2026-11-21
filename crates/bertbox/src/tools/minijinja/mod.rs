//! Globals beyond stock minijinja:
//! - `ENV`: environment variables
//! - `DATA`: the object being rendered
//! - `now`, `lipsum`, `randrange`, `cycler`, `joiner` (minijinja-contrib)
mod cli;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Error, Result, bail};
use minijinja::syntax::SyntaxConfig;
use minijinja::value::ValueKind;
use minijinja::{
    AutoEscape, Environment, Error as MError, ErrorKind, Template, UndefinedBehavior, Value,
    context,
};
use serde::{Deserialize, Deserializer};

use crate::atomic_write;
use crate::source::Source;
use crate::tools::Tool;
pub use crate::tools::minijinja::cli::Minijinja;

const STRING_TEMPLATE_NAME: &str = "<string>";
const TEMP_PREFIX: &str = ".minijinja-";

impl Tool for Minijinja {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                print_error(&error);
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Minijinja) -> Result<()> {
    let defines = Value::from(
        args.defines
            .into_iter()
            .map(|define| (define.name, define.value))
            .collect::<BTreeMap<_, _>>(),
    );

    let cwd = std::env::current_dir().context("locating the current directory")?;
    let (template_name, template_source) = match (args.template_string, args.template) {
        (Some(source), _) => (STRING_TEMPLATE_NAME.to_owned(), Some(source)),
        (None, Some(path)) => (cwd.join(path).to_string_lossy().into_owned(), None),
        (None, None) => bail!("a template string or --template is required"),
    };

    let mut env = Environment::new();
    env.set_debug(true);
    env.add_global("ENV", Value::from_pairs(std::env::vars()));
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_syntax(
        SyntaxConfig::builder()
            .keep_trailing_newline(true)
            .build()
            .context("could not configure syntax")?,
    );
    env.set_path_join_callback(move |name, parent| {
        let dir = Path::new(parent)
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(&cwd);
        let path = dir.join(name);
        fs::canonicalize(&path)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
            .into()
    });
    let loader_template_name = template_name.clone();
    env.set_loader(move |name| {
        if name == loader_template_name
            && let Some(source) = &template_source
        {
            return Ok(Some(source.clone()));
        }
        load_template(Path::new(name))
    });

    let template = env.get_template(&template_name)?;
    let inputs: Vec<Source> = match (args.inputs.is_empty(), io::stdin().is_terminal()) {
        (true, true) => Vec::new(),
        (true, false) => vec![Source::Stdin],
        (false, true) => args.inputs.into_iter().map(Source::File).collect(),
        (false, false) => bail!("--input cannot be used when stdin is not a terminal"),
    };
    match args.output {
        None => render(&template, &defines, &inputs, &mut io::stdout().lock()),
        Some(path) => {
            let temp = atomic_write::sibling(TEMP_PREFIX, &path)?;
            let file =
                File::create(&temp).with_context(|| format!("writing {}", temp.display()))?;
            let mut out = BufWriter::new(file);
            render(&template, &defines, &inputs, &mut out)?;
            out.into_inner()
                .map_err(io::IntoInnerError::into_error)
                .with_context(|| format!("writing {}", temp.display()))?;
            atomic_write::replace(temp, &path)
        }
    }
}

fn render(
    template: &Template<'_, '_>,
    defines: &Value,
    inputs: &[Source],
    out: &mut impl Write,
) -> Result<()> {
    if inputs.is_empty() {
        template.render_captured_to(defines.clone(), &mut *out)?;
        out.flush()?;
        return Ok(());
    }
    for source in inputs {
        let reader = BufReader::new(source.open()?);
        let objects = serde_json::Deserializer::from_reader(reader).into_iter::<InputObject>();
        for (index, object) in objects.enumerate() {
            let InputObject(object) = object.with_context(|| {
                let name = match source {
                    Source::Stdin => "stdin".into(),
                    Source::File(path) => path.display().to_string(),
                };
                format!("reading object {} of {name}", index + 1)
            })?;
            let ctx = context! { DATA => object, ..defines.clone() };
            template.render_captured_to(ctx, &mut *out)?;
            out.flush()?;
        }
    }
    Ok(())
}

struct InputObject(Value);

impl<'de> Deserialize<'de> for InputObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.kind() != ValueKind::Map {
            return Err(serde::de::Error::custom(format!(
                "expected a JSON object, got {}",
                value.kind()
            )));
        }
        Ok(Self(value))
    }
}

fn load_template(path: &Path) -> Result<Option<String>, MError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            Err(MError::new(ErrorKind::TemplateNotFound, "cannot find template").with_source(err))
        }
    }
}

fn print_error(err: &Error) {
    eprintln!("Error: {err}");
    if let Some(err) = err.downcast_ref::<MError>()
        && err.name().is_some()
    {
        eprintln!("{}", err.display_debug_info());
    }
    let mut source_opt = err.source();
    while let Some(source) = source_opt {
        eprintln!();
        eprintln!("caused by: {source}");
        if let Some(source) = source.downcast_ref::<MError>()
            && source.name().is_some()
        {
            eprintln!("{}", source.display_debug_info());
        }
        source_opt = source.source();
    }
}
