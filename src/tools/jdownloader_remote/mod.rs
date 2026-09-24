mod api;
mod cli;
mod model;
mod output;

use std::cmp::Reverse;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

use crate::tools::Tool;
use crate::tools::jdownloader_remote::api::{Api, Credentials};
pub use crate::tools::jdownloader_remote::cli::JdownloaderRemote;
use crate::tools::jdownloader_remote::cli::{
    Account, Captcha, Command, Config, Downloads, Extract, Grabber, Plugins, RenameTarget, Sort,
};
use crate::tools::jdownloader_remote::model::{AddLinks, LinkCollectingJob, VariantId};
use crate::tools::jdownloader_remote::output::{Cell, Format, Table};
use crate::urls::{Scheme, unique_urls};

impl Tool for JdownloaderRemote {
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

fn run(args: JdownloaderRemote) -> Result<()> {
    let credentials = match (args.username, args.password) {
        (Some(username), Some(password)) => Some(Credentials { username, password }),
        (None, None) => None,
        _ => unreachable!("clap requires --username and --password together"),
    };
    let api = Api::new(&args.url, credentials.as_ref())?;
    let format = args.format;
    let mut out = io::stdout().lock();
    match args.command {
        Command::Version => writeln!(out, "{}", api.version()?)?,
        Command::ApiHelp => writeln!(out, "{}", api.api_help()?)?,
        Command::Add { files } => add(&api, format, files, &mut out)?,
        Command::Cleanup {
            action,
            mode,
            links,
            packages,
        } => api.cleanup(&links, &packages, action, mode)?,
        Command::Downloads(command) => downloads(&api, format, command, &mut out)?,
        Command::Grabber(command) => grabber(&api, format, command, &mut out)?,
        Command::Captcha(command) => captcha(&api, format, command, &mut out)?,
        Command::Account(command) => account(&api, format, command, &mut out)?,
        Command::Extract(command) => extract(&api, format, command, &mut out)?,
        Command::Config(command) => config(&api, format, command, &mut out)?,
        Command::Plugins(command) => plugins(&api, format, command, &mut out)?,
    }
    out.flush()?;
    Ok(())
}

fn add(api: &Api, format: Format, files: Vec<PathBuf>, out: &mut impl Write) -> Result<()> {
    let mut stdin = io::stdin().lock();
    let mut data = Vec::new();
    if !stdin.is_terminal() {
        stdin.read_to_end(&mut data).context("reading stdin")?;
    }
    let text = String::from_utf8_lossy(&data);
    let mut urls = Vec::new();
    for url in unique_urls(&text) {
        let url = url?;
        if [Scheme::FTP, Scheme::HTTP, Scheme::HTTPS].contains(url.scheme()) {
            urls.push(url);
        }
    }
    if files.is_empty() && urls.is_empty() {
        bail!("no container files or URLs to add");
    }
    let mut added = Vec::new();
    for path in files {
        add_container(api, &path, false)?;
        added.push(path.display().to_string());
    }
    if !urls.is_empty() {
        api.add_links(&AddLinks {
            links: urls.iter().map(|url| &**url).collect::<Vec<_>>().join("\n"),
            autostart: true,
            package_name: None,
            destination_folder: None,
            extract_password: None,
            download_password: None,
        })?;
        added.extend(urls.iter().map(|url| (**url).to_owned()));
    }
    let rows = added
        .iter()
        .map(|item| vec![Cell::from(item.as_str())])
        .collect();
    output::write_items(out, format, &added, Table::new(&["added"], rows))
}

fn downloads(api: &Api, format: Format, command: Downloads, out: &mut impl Write) -> Result<()> {
    match command {
        Downloads::AddLinks {
            package,
            folder,
            extract_password,
            download_password,
            no_start,
            urls,
        } => {
            let job = api.add_links(&AddLinks {
                links: urls.join("\n"),
                autostart: !no_start,
                package_name: package,
                destination_folder: folder,
                extract_password,
                download_password,
            })?;
            write_job(out, format, &job)
        }
        Downloads::AddContainer { rm, files } => {
            for path in files {
                let job = add_container(api, &path, rm)?;
                write_job(out, format, &job)?;
            }
            Ok(())
        }
        Downloads::List {
            running,
            status,
            sort,
        } => list_downloads(api, format, running, status.as_deref(), sort, out),
        Downloads::Packages => {
            let packages = api.packages()?;
            let rows = packages
                .iter()
                .map(|p| {
                    vec![
                        Cell::int(p.uuid),
                        Cell::from(p.name.as_str()),
                        Cell::from(format!("{:>3}%", p.percent())),
                        Cell::from(p.status.as_str()),
                        Cell::from(p.save_to.as_str()),
                    ]
                })
                .collect();
            let table = Table::new(&["uuid", "name", "percent", "status", "saveTo"], rows);
            output::write_items(out, format, &packages, table)
        }
        Downloads::Start => api.start(),
        Downloads::Stop => api.stop(),
        Downloads::Pause => api.pause(true),
        Downloads::Unpause => api.pause(false),
        Downloads::Remove(selection) => api.remove(&selection.links, &selection.packages),
        Downloads::Reset(selection) => api.reset(&selection.links, &selection.packages),
        Downloads::Resume(selection) => api.resume(&selection.links, &selection.packages),
        Downloads::Enable(selection) => {
            api.set_enabled(true, &selection.links, &selection.packages)
        }
        Downloads::Disable(selection) => {
            api.set_enabled(false, &selection.links, &selection.packages)
        }
        Downloads::Rename { target, name } => match target {
            RenameTarget {
                link: Some(link), ..
            } => api.rename_link(link, &name),
            RenameTarget {
                package: Some(package),
                ..
            } => api.rename_package(package, &name),
            RenameTarget { .. } => unreachable!("clap requires --link or --package"),
        },
        Downloads::SetFolder { packages, dir } => api.set_download_folder(&dir, &packages),
        Downloads::Priority {
            selection,
            priority,
        } => api.set_priority(priority, &selection.links, &selection.packages),
    }
}

fn add_container(api: &Api, path: &Path, rm: bool) -> Result<LinkCollectingJob> {
    let kind = path
        .extension()
        .and_then(|extension| extension.to_str())
        .with_context(|| format!("file has no extension: {}", path.display()))?;
    let content = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let job = api.add_container(kind, &content)?;
    if rm {
        fs::remove_file(path).with_context(|| format!("cannot delete {}", path.display()))?;
    }
    Ok(job)
}

fn list_downloads(
    api: &Api,
    format: Format,
    running: bool,
    status: Option<&str>,
    sort: Sort,
    out: &mut impl Write,
) -> Result<()> {
    let mut links = api.downloads()?;
    links.retain(|link| {
        (!running || link.running) && status.is_none_or(|status| link.status.contains(status))
    });
    match sort {
        Sort::Name => links.sort_by(|a, b| a.name.cmp(&b.name)),
        Sort::Status => links.sort_by(|a, b| a.status.cmp(&b.status)),
        Sort::Percent => links.sort_by_key(|link| Reverse(link.percent())),
        Sort::Speed => links.sort_by_key(|link| Reverse(link.speed)),
        Sort::Eta => links.sort_by_key(|link| (link.eta.is_none(), link.eta)),
    }
    let rows = links
        .iter()
        .map(|l| {
            vec![
                Cell::from(format!("{:>3}%", l.percent())),
                Cell::from(l.status.as_str()),
                Cell::from(l.name.as_str()),
                Cell::from(l.speed),
                Cell::from(l.eta),
                Cell::int(l.uuid),
                Cell::int(l.package_uuid),
            ]
        })
        .collect();
    let headers = &[
        "percent", "status", "name", "speed", "eta", "uuid", "package",
    ];
    let table = Table::new(headers, rows).text_columns(3);
    output::write_items(out, format, &links, table)
}

fn grabber(api: &Api, format: Format, command: Grabber, out: &mut impl Write) -> Result<()> {
    match command {
        Grabber::List => {
            let links = api.grabber_links()?;
            let rows = links
                .iter()
                .map(|l| {
                    vec![
                        Cell::int(l.uuid),
                        Cell::int(l.package_uuid),
                        Cell::from(l.availability.as_str()),
                        Cell::from(l.name.as_str()),
                        Cell::from(l.url.as_str()),
                    ]
                })
                .collect();
            let headers = &["uuid", "package", "availability", "name", "url"];
            output::write_items(out, format, &links, Table::new(headers, rows))
        }
        Grabber::Confirm {
            selection,
            no_start,
        } => {
            api.confirm(&selection.links, &selection.packages)?;
            if !no_start {
                api.start()?;
            }
            Ok(())
        }
        Grabber::Clear => api.clear_grabber(),
        Grabber::Variants { link_id } => {
            let variants = api.variants(link_id)?;
            let rows = variants
                .iter()
                .map(|v| {
                    vec![
                        Cell::from(v.id.as_ref().map(VariantId::as_str)),
                        Cell::from(v.name.as_deref()),
                    ]
                })
                .collect();
            output::write_items(out, format, &variants, Table::new(&["id", "name"], rows))
        }
        Grabber::SetVariant {
            link_id,
            variant_id,
        } => api.set_variant(link_id, &variant_id),
    }
}

fn captcha(api: &Api, format: Format, command: Captcha, out: &mut impl Write) -> Result<()> {
    match command {
        Captcha::List => {
            let captchas = api.captchas()?;
            let rows = captchas
                .iter()
                .map(|c| {
                    vec![
                        Cell::int(c.id),
                        Cell::from(c.hoster.as_deref()),
                        Cell::from(c.kind.as_deref()),
                        Cell::from(c.remaining),
                        Cell::from(c.explain.as_deref()),
                    ]
                })
                .collect();
            let headers = &["id", "hoster", "type", "remaining", "explain"];
            output::write_items(out, format, &captchas, Table::new(headers, rows))
        }
        Captcha::Image { output, captcha_id } => {
            let image = api.captcha_image(captcha_id)?;
            let path = output.unwrap_or_else(|| {
                PathBuf::from(format!("captcha-{captcha_id}.{}", image.extension))
            });
            fs::write(&path, &image.data).with_context(|| format!("writing {}", path.display()))
        }
        Captcha::Solve { captcha_id, answer } => api.solve_captcha(captcha_id, &answer),
        Captcha::Skip { captcha_id } => api.skip_captcha(captcha_id),
    }
}

fn account(api: &Api, format: Format, command: Account, out: &mut impl Write) -> Result<()> {
    match command {
        Account::List => {
            let accounts = api.accounts()?;
            let rows = accounts
                .iter()
                .map(|a| {
                    vec![
                        Cell::int(a.uuid),
                        Cell::from(a.hostname.as_str()),
                        Cell::from(a.username.as_str()),
                        Cell::from(a.enabled),
                        Cell::from(a.valid),
                        Cell::from(a.error()),
                    ]
                })
                .collect();
            let headers = &["uuid", "hostname", "username", "enabled", "valid", "error"];
            output::write_items(out, format, &accounts, Table::new(headers, rows))
        }
        Account::Add {
            password_stdin,
            hoster,
            username,
        } => {
            let password = if password_stdin {
                let mut password = String::new();
                io::stdin()
                    .read_to_string(&mut password)
                    .context("reading the password from stdin")?;
                password.trim_end_matches('\n').to_owned()
            } else {
                rpassword::prompt_password("Password: ").context("reading the password")?
            };
            api.add_account(&hoster, &username, &password)
        }
        Account::Remove(accounts) => api.remove_accounts(&accounts.ids),
        Account::Enable(accounts) => api.enable_accounts(&accounts.ids),
        Account::Disable(accounts) => api.disable_accounts(&accounts.ids),
        Account::Refresh(accounts) => api.refresh_accounts(&accounts.ids),
    }
}

fn extract(api: &Api, format: Format, command: Extract, out: &mut impl Write) -> Result<()> {
    match command {
        Extract::List => {
            let archives = api.archives()?;
            let rows = archives
                .iter()
                .map(|a| {
                    vec![
                        Cell::from(a.archive_id.as_str()),
                        Cell::from(a.controller_status.as_deref()),
                        Cell::from(a.kind.as_deref()),
                        Cell::from(a.archive_name.as_deref()),
                    ]
                })
                .collect();
            let headers = &["id", "status", "type", "name"];
            output::write_items(out, format, &archives, Table::new(headers, rows))
        }
        Extract::Start(selection) => api.start_extraction(&selection.links, &selection.packages),
        Extract::SetPassword {
            archive_id,
            password,
        } => api.set_archive_password(&archive_id, password),
    }
}

fn config(api: &Api, format: Format, command: Config, out: &mut impl Write) -> Result<()> {
    match command {
        Config::List { pattern } => {
            let entries = api.config_entries(pattern.as_deref())?;
            let rows = entries
                .iter()
                .map(|e| {
                    vec![
                        Cell::from(e.interface_name.as_str()),
                        Cell::from(e.storage.as_deref()),
                        Cell::from(e.key.as_str()),
                        Cell::from(e.value.to_string()),
                    ]
                })
                .collect();
            let headers = &["interface", "storage", "key", "value"];
            output::write_items(out, format, &entries, Table::new(headers, rows))
        }
        Config::Get { interface, key } => {
            let value = api.config_get(&interface, &key)?;
            match format {
                Format::Json => output::write_json(out, &value),
                Format::Text | Format::Table => {
                    writeln!(out, "{value}")?;
                    Ok(())
                }
            }
        }
        Config::Set {
            interface,
            key,
            value,
        } => api.config_set(&interface, &key, &value),
    }
}

fn plugins(api: &Api, format: Format, command: Plugins, out: &mut impl Write) -> Result<()> {
    match command {
        Plugins::List { pattern } => {
            let mut plugins = api.plugins()?;
            if let Some(pattern) = pattern {
                plugins.retain(|plugin| plugin.display_name.contains(&pattern));
            }
            let rows = plugins
                .iter()
                .map(|p| {
                    vec![
                        Cell::from(p.display_name.as_str()),
                        Cell::from(p.version.as_str()),
                        Cell::from(p.class_name.as_str()),
                    ]
                })
                .collect();
            let headers = &["name", "version", "class"];
            output::write_items(out, format, &plugins, Table::new(headers, rows))
        }
        Plugins::Settings { hoster } => {
            let entries = api.plugin_settings(&hoster)?;
            let rows = entries
                .iter()
                .map(|e| {
                    vec![
                        Cell::from(e.interface_name.as_str()),
                        Cell::from(e.storage.as_str()),
                        Cell::from(e.key.as_str()),
                        Cell::from(e.value.to_string()),
                    ]
                })
                .collect();
            let headers = &["interface", "storage", "key", "value"];
            output::write_items(out, format, &entries, Table::new(headers, rows))
        }
    }
}

fn write_job(out: &mut impl Write, format: Format, job: &LinkCollectingJob) -> Result<()> {
    match format {
        Format::Json => output::write_json(out, job),
        Format::Text | Format::Table => {
            writeln!(out, "{}", job.id)?;
            Ok(())
        }
    }
}
