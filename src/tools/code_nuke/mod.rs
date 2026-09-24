mod cli;
mod rule;

use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use ast_grep_config::{Fixer, GlobalRules, RuleConfig, from_yaml_string};
use ast_grep_language::{LanguageExt, Rust};
use clap::Parser;
use ignore::WalkBuilder;
use serde_json::Value;
use similar::TextDiff;
use uncomment::cli::ProcessArgs;
use uncomment::config::ConfigManager;
use uncomment::languages::LanguageRegistry;
use uncomment::processor::{OutputWriter, ProcessedFile, Processor};

use crate::pool;
use crate::tools::Tool;
pub use crate::tools::code_nuke::cli::CodeNuke;
use crate::tools::code_nuke::cli::{Passthrough, Target};

impl Tool for CodeNuke {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: CodeNuke) -> Result<()> {
    let (result, apply) = match args.target {
        Target::Stmt {
            directory,
            pattern,
            kinds,
            apply,
        } => (
            ast_grep(&rule::drop_stmt(&pattern, &kinds), &directory, apply),
            apply,
        ),
        Target::Tests { path, apply } => (ast_grep(&rule::drop_tests(), &path, apply), apply),
        Target::Kinds { file, line } => return kinds(&file, line),
        Target::Comments { path, passthrough } => {
            let apply = passthrough.apply;
            (uncomment(&path, passthrough), apply)
        }
    };
    if !apply {
        eprintln!("warning: dry run, no files were changed. pass -a to apply");
    }
    result
}

/// Prints the named nodes enclosing the first code on `line`, innermost first.
fn kinds(file: &Path, line: NonZeroUsize) -> Result<()> {
    let source =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let mut start = 0;
    let mut text = None;
    for (number, content) in source.split_inclusive('\n').enumerate() {
        if number + 1 == line.get() {
            text = Some(content);
            break;
        }
        start += content.len();
    }
    let text = text.with_context(|| format!("{} has no line {line}", file.display()))?;
    let indent = text.len() - text.trim_start().len();
    if text.trim().is_empty() {
        bail!("line {line} is blank");
    }
    let offset = start + indent;

    let grep = Rust.ast_grep(&source);
    let mut node = grep.root();
    loop {
        let child = node
            .named_children()
            .find(|child| child.range().contains(&offset));
        let Some(child) = child else { break };
        node = child;
    }
    for node in std::iter::once(node.clone()).chain(node.ancestors()) {
        let preview = node.text();
        let preview = preview.lines().next().unwrap_or_default().trim();
        println!(
            "{:<24} {:>5}-{:<5} {preview}",
            node.kind(),
            node.start_pos().line() + 1,
            node.end_pos().line() + 1,
        );
    }
    Ok(())
}

fn ast_grep(rule: &Value, path: &Path, apply: bool) -> Result<()> {
    let rule = rule.to_string();
    let rule = from_yaml_string::<Rust>(&rule, &GlobalRules::default())
        .context("parsing ast-grep rule")?
        .pop()
        .context("ast-grep rule is empty")?;
    let fixer = rule.fixer.first().context("ast-grep rule has no fix")?;

    let mut files = Vec::new();
    for entry in WalkBuilder::new(path).build() {
        let entry = entry?;
        let file = entry.path();
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && file.extension().is_some_and(|extension| extension == "rs")
        {
            files.push(file.to_path_buf());
        }
    }

    let mut rewritten = Vec::new();
    let mut failed = 0usize;
    pool::run(
        files,
        pool::jobs(0)?,
        |file: &PathBuf| -> Result<Option<(String, String)>> {
            let source = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let output = rewrite(&source, &rule, fixer)?;
            Ok((output != source).then_some((source, output)))
        },
        |file, result| {
            match result {
                Ok(Some(contents)) => rewritten.push((file, contents)),
                Ok(None) => {}
                Err(error) => {
                    eprintln!("error: processing {}: {error:#}", file.display());
                    failed += 1;
                }
            }
            Ok(())
        },
    )?;
    rewritten.sort_by(|a, b| a.0.cmp(&b.0));

    for (file, (source, output)) in &rewritten {
        if apply {
            std::fs::write(file, output).with_context(|| format!("writing {}", file.display()))?;
        } else {
            let name = file.display().to_string();
            print!(
                "{}",
                TextDiff::from_lines(source, output)
                    .unified_diff()
                    .header(&name, &name)
            );
        }
    }
    if failed > 0 {
        bail!("{failed} file(s) failed to process");
    }
    Ok(())
}

/// Applies `fixer` at every match of `rule`, skipping matches that overlap an earlier one.
fn rewrite(source: &str, rule: &RuleConfig<Rust>, fixer: &Fixer) -> Result<String> {
    let grep = Rust.ast_grep(source);
    let mut output = String::with_capacity(source.len());
    let mut end = 0;
    for found in grep.root().find_all(&rule.matcher) {
        let edit = found.make_edit(&rule.matcher, fixer);
        if edit.position < end {
            continue;
        }
        output.push_str(&source[end..edit.position]);
        let fix = std::str::from_utf8(&edit.inserted_text).context("fix is not valid UTF-8")?;
        output.push_str(fix);
        end = edit.position + edit.deleted_length;
    }
    output.push_str(&source[end..]);
    Ok(output)
}

fn uncomment(path: &Path, passthrough: Passthrough) -> Result<()> {
    let mut argv: Vec<OsString> = vec!["uncomment".into(), path.into()];
    argv.extend(
        [
            "-j",
            "0",
            "--remove-todo",
            "--remove-fixme",
            "--remove-doc",
            "--no-default-ignores",
        ]
        .map(OsString::from),
    );
    if !passthrough.apply {
        argv.extend(["--dry-run", "--diff"].map(OsString::from));
    }
    argv.extend(passthrough.rest);
    let args = ProcessArgs::try_parse_from(argv).unwrap_or_else(|error| error.exit());
    let options = args.processing_options();

    let cwd = std::env::current_dir().context("getting current directory")?;
    let mut config = match &args.config {
        Some(file) => ConfigManager::from_config_file(&cwd, file)?,
        None => ConfigManager::new(&cwd)?,
    };
    config.discover_language_sources(&args.paths, options.respect_gitignore);
    let mut registry = LanguageRegistry::new();
    registry.register_configured_languages(&config.get_all_languages());
    let excludes = config.exclude_set(&args.exclude)?;

    let mut files = Vec::new();
    for root in &args.paths {
        let walker = WalkBuilder::new(root)
            .standard_filters(options.respect_gitignore)
            .hidden(false)
            .require_git(false)
            .filter_entry(excludes.walk_filter())
            .build();
        for entry in walker {
            let entry = entry?;
            let file = entry.path();
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && !excludes.is_excluded(file)
                && registry.detect_language(file).is_some()
            {
                files.push(file.to_path_buf());
            }
        }
    }
    files.sort();
    files.dedup();
    let total = files.len();

    let mut processed: Vec<ProcessedFile> = Vec::with_capacity(total);
    let mut failed = 0usize;
    pool::run(
        files,
        pool::jobs(args.threads)?,
        |file: &PathBuf| {
            Processor::new_with_config(&config).process_file_with_config(
                file,
                &config,
                Some(&options),
            )
        },
        |file, result| {
            match result {
                Ok(mut done) => {
                    done.modified = done.original_content != done.processed_content;
                    processed.push(done);
                }
                Err(error) => {
                    eprintln!("error: processing {}: {error:#}", file.display());
                    failed += 1;
                }
            }
            Ok(())
        },
    )?;
    if config.deferred_config_error().is_some() {
        bail!("uncomment configuration was rejected, no file was modified");
    }
    processed.sort_by(|a, b| a.path.cmp(&b.path));

    let writer = OutputWriter::new(options.dry_run, args.verbose, options.show_diff, args.quiet);
    let (mut modified, mut removed) = (0, 0);
    for file in &processed {
        if file.modified {
            modified += 1;
            removed += file.comments_removed;
        }
        writer.write_file(file)?;
    }
    writer.print_summary(total, modified, removed);
    if failed > 0 {
        bail!("{failed} file(s) failed to process");
    }
    Ok(())
}
