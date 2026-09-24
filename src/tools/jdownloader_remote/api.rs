use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use reqwest::Url;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::http;
use crate::tools::jdownloader_remote::model::{
    Account, AccountId, AddLinks, ArchiveId, ArchiveSettings, ArchiveStatus, CaptchaId,
    CaptchaImage, CaptchaJob, CleanupAction, CleanupMode, CleanupSelection, ConfigEntry,
    CrawledLink, DownloadLink, DupesAction, FilePackage, LinkCollectingJob, LinkId, LinkVariant,
    PackageId, Plugin, PluginConfigEntry, Priority, VariantId,
};

pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct ErrorResponse {
    src: String,
    #[serde(rename = "type")]
    kind: String,
}

pub struct Api {
    client: Client,
    base: Url,
}

impl Api {
    pub fn new(base: &str, credentials: Option<&Credentials>) -> Result<Self> {
        let mut base = Url::parse(base).with_context(|| format!("parsing the url {base:?}"))?;
        base.path_segments_mut()
            .map_err(|()| anyhow!("the jdownloader url cannot hold a path"))?
            .pop_if_empty();
        let mut headers = HeaderMap::new();
        if let Some(Credentials { username, password }) = credentials {
            let token = STANDARD.encode(format!("{username}:{password}"));
            let mut value = HeaderValue::from_str(&format!("Basic {token}"))
                .context("the credentials are not a valid header value")?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let client = http::client_builder()?
            .default_headers(headers)
            .build()
            .context("building the http client")?;
        Ok(Self { client, base })
    }

    pub fn version(&self) -> Result<i64> {
        self.call("jd/version", json!([]))
    }

    pub fn api_help(&self) -> Result<String> {
        self.fetch(self.client.get(self.url("help")))
    }

    pub fn start(&self) -> Result<()> {
        self.send("downloadcontroller/start", json!([]))
    }

    pub fn stop(&self) -> Result<()> {
        self.send("downloadcontroller/stop", json!([]))
    }

    pub fn pause(&self, paused: bool) -> Result<()> {
        self.send("downloadcontroller/pause", json!([paused]))
    }

    pub fn add_links(&self, links: &AddLinks) -> Result<LinkCollectingJob> {
        self.call("linkgrabberv2/addLinks", json!([links]))
    }

    pub fn add_container(&self, kind: &str, content: &[u8]) -> Result<LinkCollectingJob> {
        let url = format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(content)
        );
        self.call("linkgrabberv2/addContainer", json!([kind, url]))
    }

    pub fn downloads(&self) -> Result<Vec<DownloadLink>> {
        let query = json!({
            "addedDate": true,
            "bytesLoaded": true,
            "bytesTotal": true,
            "enabled": true,
            "eta": true,
            "finished": true,
            "host": true,
            "jobUUID": true,
            "priority": true,
            "running": true,
            "skipped": true,
            "speed": true,
            "status": true,
            "url": true,
        });
        self.call("downloadsV2/queryLinks", json!([query]))
    }

    pub fn packages(&self) -> Result<Vec<FilePackage>> {
        let query = json!({
            "bytesLoaded": true,
            "bytesTotal": true,
            "childCount": true,
            "enabled": true,
            "eta": true,
            "finished": true,
            "hosts": true,
            "priority": true,
            "running": true,
            "saveTo": true,
            "speed": true,
            "status": true,
        });
        self.call("downloadsV2/queryPackages", json!([query]))
    }

    pub fn remove(&self, links: &[LinkId], packages: &[PackageId]) -> Result<()> {
        self.send("downloadsV2/removeLinks", json!([links, packages]))
    }

    pub fn cleanup(
        &self,
        links: &[LinkId],
        packages: &[PackageId],
        action: CleanupAction,
        mode: CleanupMode,
    ) -> Result<()> {
        let selection = if links.is_empty() && packages.is_empty() {
            CleanupSelection::All
        } else {
            CleanupSelection::Selected
        };
        self.send(
            "downloadsV2/cleanup",
            json!([links, packages, action, mode, selection]),
        )
    }

    pub fn reset(&self, links: &[LinkId], packages: &[PackageId]) -> Result<()> {
        self.send("downloadsV2/resetLinks", json!([links, packages]))
    }

    pub fn resume(&self, links: &[LinkId], packages: &[PackageId]) -> Result<()> {
        self.send("downloadsV2/resumeLinks", json!([links, packages]))
    }

    pub fn set_enabled(
        &self,
        enabled: bool,
        links: &[LinkId],
        packages: &[PackageId],
    ) -> Result<()> {
        self.send("downloadsV2/setEnabled", json!([enabled, links, packages]))
    }

    pub fn rename_link(&self, link: LinkId, name: &str) -> Result<()> {
        self.send("downloadsV2/renameLink", json!([link, name]))
    }

    pub fn rename_package(&self, package: PackageId, name: &str) -> Result<()> {
        self.send("downloadsV2/renamePackage", json!([package, name]))
    }

    pub fn set_download_folder(&self, folder: &str, packages: &[PackageId]) -> Result<()> {
        self.send(
            "downloadsV2/setDownloadDirectory",
            json!([folder, packages]),
        )
    }

    pub fn set_priority(
        &self,
        priority: Priority,
        links: &[LinkId],
        packages: &[PackageId],
    ) -> Result<()> {
        self.send(
            "downloadsV2/setPriority",
            json!([priority, links, packages]),
        )
    }

    pub fn grabber_links(&self) -> Result<Vec<CrawledLink>> {
        let query = json!({
            "availability": true,
            "bytesTotal": true,
            "enabled": true,
            "host": true,
            "jobUUID": true,
            "url": true,
            "variants": true,
            "variantID": true,
            "variantName": true,
        });
        self.call("linkgrabberv2/queryLinks", json!([query]))
    }

    pub fn confirm(&self, links: &[LinkId], packages: &[PackageId]) -> Result<()> {
        self.send("linkgrabberv2/moveToDownloadlist", json!([links, packages]))
    }

    pub fn clear_grabber(&self) -> Result<()> {
        self.send("linkgrabberv2/clearList", json!([]))
    }

    pub fn variants(&self, link: LinkId) -> Result<Vec<LinkVariant>> {
        self.call("linkgrabberv2/getVariants", json!([link]))
    }

    pub fn set_variant(&self, link: LinkId, variant: &VariantId) -> Result<()> {
        self.send("linkgrabberv2/setVariant", json!([link, variant]))
    }

    pub fn captchas(&self) -> Result<Vec<CaptchaJob>> {
        self.call("captcha/list", json!([]))
    }

    pub fn captcha_image(&self, captcha: CaptchaId) -> Result<CaptchaImage> {
        let url: String = self.call("captcha/get", json!([captcha]))?;
        CaptchaImage::from_data_url(&url)
    }

    pub fn solve_captcha(&self, captcha: CaptchaId, answer: &str) -> Result<()> {
        self.send("captcha/solve", json!([captcha, answer]))
    }

    pub fn skip_captcha(&self, captcha: CaptchaId) -> Result<()> {
        self.send("captcha/skip", json!([captcha, "SINGLE"]))
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let query = json!({
            "enabled": true,
            "error": true,
            "trafficLeft": true,
            "trafficMax": true,
            "userName": true,
            "valid": true,
            "validUntil": true,
        });
        self.call("accountsV2/listAccounts", json!([query]))
    }

    pub fn add_account(&self, hoster: &str, username: &str, password: &str) -> Result<()> {
        self.send("accountsV2/addAccount", json!([hoster, username, password]))
    }

    pub fn remove_accounts(&self, accounts: &[AccountId]) -> Result<()> {
        self.send("accountsV2/removeAccounts", json!([accounts]))
    }

    pub fn enable_accounts(&self, accounts: &[AccountId]) -> Result<()> {
        self.send("accountsV2/enableAccounts", json!([accounts]))
    }

    pub fn disable_accounts(&self, accounts: &[AccountId]) -> Result<()> {
        self.send("accountsV2/disableAccounts", json!([accounts]))
    }

    pub fn refresh_accounts(&self, accounts: &[AccountId]) -> Result<()> {
        self.send("accountsV2/refreshAccounts", json!([accounts, true]))
    }

    pub fn archives(&self) -> Result<Vec<ArchiveStatus>> {
        self.call("extraction/getArchiveInfo", json!([[], []]))
    }

    pub fn start_extraction(&self, links: &[LinkId], packages: &[PackageId]) -> Result<()> {
        self.send("extraction/startExtractionNow", json!([links, packages]))
    }

    pub fn set_archive_password(&self, archive: &ArchiveId, password: String) -> Result<()> {
        let settings: Vec<ArchiveSettings> =
            self.call("extraction/getArchiveSettings", json!([[archive]]))?;
        let Some(settings) = settings.into_iter().next() else {
            bail!("archive not found: {archive}");
        };
        let settings = settings.with_password(password);
        self.send("extraction/setArchiveSettings", json!([archive, settings]))
    }

    pub fn config_entries(&self, pattern: Option<&str>) -> Result<Vec<ConfigEntry>> {
        let mut query = json!({ "values": true });
        if let Some(pattern) = pattern {
            query["pattern"] = json!(format!(".*{}.*", regex::escape(pattern)));
        }
        self.call("config/query", json!([query]))
    }

    pub fn config_get(&self, interface: &str, key: &str) -> Result<Value> {
        self.call("config/get", json!([interface, null, key]))
    }

    pub fn config_set(&self, interface: &str, key: &str, value: &Value) -> Result<()> {
        self.send("config/set", json!([interface, null, key, value]))
    }

    pub fn set_dupes_action(&self, action: DupesAction) -> Result<()> {
        self.config_set(
            "org.jdownloader.gui.views.linkgrabber.addlinksdialog.LinkgrabberSettings",
            "DefaultOnAddedDupesLinksAction",
            &json!(action),
        )
    }

    pub fn plugins(&self) -> Result<Vec<Plugin>> {
        self.call(
            "plugins/list",
            json!([{ "pattern": true, "version": true }]),
        )
    }

    pub fn plugin_settings(&self, hoster: &str) -> Result<Vec<PluginConfigEntry>> {
        let query = json!({
            "pattern": format!(".*{}.*", regex::escape(hoster)),
            "values": true,
        });
        self.call("plugins/query", json!([query]))
    }

    fn url(&self, path: &str) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("the base url was checked to hold a path")
            .extend(path.split('/'));
        url
    }

    fn post(&self, path: &str, params: Value) -> Result<RequestBuilder> {
        let body = serde_json::to_vec(&json!({ "params": params }))
            .context("encoding the request body")?;
        Ok(self
            .client
            .post(self.url(path))
            .header(CONTENT_TYPE, "application/json")
            .body(body))
    }

    fn send(&self, path: &str, params: Value) -> Result<()> {
        self.fetch(self.post(path, params)?).map(drop)
    }

    fn call<T: DeserializeOwned>(&self, path: &str, params: Value) -> Result<T> {
        let body = self.fetch(self.post(path, params)?)?;
        let envelope: Envelope<T> = serde_json::from_str(&body)
            .with_context(|| format!("decoding the response to {path}"))?;
        Ok(envelope.data)
    }

    fn fetch(&self, request: RequestBuilder) -> Result<String> {
        let request = request.build().context("building a request")?;
        let path = request.url().path().to_owned();
        let response = self
            .client
            .execute(request)
            .with_context(|| format!("requesting {path}"))?;
        let status = response.status();
        let body = response
            .text()
            .with_context(|| format!("reading the response to {path}"))?;
        if status.is_success() {
            return Ok(body);
        }
        match serde_json::from_str::<ErrorResponse>(&body) {
            Ok(error) => bail!("{path}: {} {}", error.src, error.kind),
            Err(_) => bail!("{path}: HTTP {}", status.as_u16()),
        }
    }
}
