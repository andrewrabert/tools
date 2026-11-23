//! A blocking client for JDownloader's local API.

mod model;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bertbox_http::{BaseUrl, Reply, json_body, sensitive_header};
use reqwest::Url;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::model::CleanupSelection;
pub use crate::model::{
    Account, AccountId, AddLinks, AdvancedStatus, ArchiveId, ArchiveSettings, ArchiveStatus,
    CaptchaId, CaptchaImage, CaptchaJob, CleanupAction, CleanupMode, ConfigEntry, CrawledLink,
    DownloadLink, DupesAction, FilePackage, FinalLinkState, JobId, LinkCollectingJob, LinkId,
    LinkState, LinkVariant, PackageId, Plugin, PluginConfigEntry, Priority, SkipReason,
    StatusEntry, VariantId,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] bertbox_http::Error),
    /// The server answered with its own error.
    #[error("{url}: {src} {kind}")]
    Server {
        url: Box<Url>,
        src: String,
        kind: String,
    },
    /// The server failed without saying why.
    #[error("{url}: HTTP {status}")]
    Status { url: Box<Url>, status: u16 },
    #[error("archive not found: {0}")]
    ArchiveNotFound(String),
    #[error("decoding the captcha image")]
    CaptchaImage(#[source] base64::DecodeError),
}

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
    base: BaseUrl,
}

impl Api {
    /// A client for the API at `base`, authenticating when given credentials.
    pub fn new(base: Url, credentials: Option<&Credentials>) -> Result<Self, Error> {
        let base = BaseUrl::new(base, &[])?;
        let mut headers = HeaderMap::new();
        if let Some(Credentials { username, password }) = credentials {
            let token = STANDARD.encode(format!("{username}:{password}"));
            let value = sensitive_header("the credentials", &format!("Basic {token}"))?;
            headers.insert(AUTHORIZATION, value);
        }
        let client = bertbox_http::client_builder()?
            .default_headers(headers)
            .build()
            .map_err(bertbox_http::Error::Client)?;
        Ok(Self { client, base })
    }

    pub fn version(&self) -> Result<i64, Error> {
        self.call("jd/version", json!([]))
    }

    pub fn api_help(&self) -> Result<String, Error> {
        Ok(self.fetch(self.client.get(self.url("help")))?.body)
    }

    pub fn start(&self) -> Result<(), Error> {
        self.send("downloadcontroller/start", json!([]))
    }

    pub fn stop(&self) -> Result<(), Error> {
        self.send("downloadcontroller/stop", json!([]))
    }

    pub fn pause(&self, paused: bool) -> Result<(), Error> {
        self.send("downloadcontroller/pause", json!([paused]))
    }

    pub fn add_links(&self, links: &AddLinks) -> Result<LinkCollectingJob, Error> {
        self.call("linkgrabberv2/addLinks", json!([links]))
    }

    pub fn add_container(&self, kind: &str, content: &[u8]) -> Result<LinkCollectingJob, Error> {
        let url = format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(content)
        );
        self.call("linkgrabberv2/addContainer", json!([kind, url]))
    }

    pub fn downloads(&self) -> Result<Vec<DownloadLink>, Error> {
        let query = json!({
            "addedDate": true,
            "advancedStatus": true,
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

    pub fn packages(&self) -> Result<Vec<FilePackage>, Error> {
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

    pub fn remove(&self, links: &[LinkId], packages: &[PackageId]) -> Result<(), Error> {
        self.send("downloadsV2/removeLinks", json!([links, packages]))
    }

    pub fn cleanup(
        &self,
        links: &[LinkId],
        packages: &[PackageId],
        action: CleanupAction,
        mode: CleanupMode,
    ) -> Result<(), Error> {
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

    pub fn reset(&self, links: &[LinkId], packages: &[PackageId]) -> Result<(), Error> {
        self.send("downloadsV2/resetLinks", json!([links, packages]))
    }

    pub fn resume(&self, links: &[LinkId], packages: &[PackageId]) -> Result<(), Error> {
        self.send("downloadsV2/resumeLinks", json!([links, packages]))
    }

    pub fn set_enabled(
        &self,
        enabled: bool,
        links: &[LinkId],
        packages: &[PackageId],
    ) -> Result<(), Error> {
        self.send("downloadsV2/setEnabled", json!([enabled, links, packages]))
    }

    pub fn rename_link(&self, link: LinkId, name: &str) -> Result<(), Error> {
        self.send("downloadsV2/renameLink", json!([link, name]))
    }

    pub fn rename_package(&self, package: PackageId, name: &str) -> Result<(), Error> {
        self.send("downloadsV2/renamePackage", json!([package, name]))
    }

    pub fn set_download_folder(&self, folder: &str, packages: &[PackageId]) -> Result<(), Error> {
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
    ) -> Result<(), Error> {
        self.send(
            "downloadsV2/setPriority",
            json!([priority, links, packages]),
        )
    }

    pub fn grabber_links(&self) -> Result<Vec<CrawledLink>, Error> {
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

    pub fn confirm(&self, links: &[LinkId], packages: &[PackageId]) -> Result<(), Error> {
        self.send("linkgrabberv2/moveToDownloadlist", json!([links, packages]))
    }

    pub fn clear_grabber(&self) -> Result<(), Error> {
        self.send("linkgrabberv2/clearList", json!([]))
    }

    pub fn variants(&self, link: LinkId) -> Result<Vec<LinkVariant>, Error> {
        self.call("linkgrabberv2/getVariants", json!([link]))
    }

    pub fn set_variant(&self, link: LinkId, variant: &VariantId) -> Result<(), Error> {
        self.send("linkgrabberv2/setVariant", json!([link, variant]))
    }

    pub fn captchas(&self) -> Result<Vec<CaptchaJob>, Error> {
        self.call("captcha/list", json!([]))
    }

    pub fn captcha_image(&self, captcha: CaptchaId) -> Result<CaptchaImage, Error> {
        let url: String = self.call("captcha/get", json!([captcha]))?;
        CaptchaImage::from_data_url(&url)
    }

    pub fn solve_captcha(&self, captcha: CaptchaId, answer: &str) -> Result<(), Error> {
        self.send("captcha/solve", json!([captcha, answer]))
    }

    pub fn skip_captcha(&self, captcha: CaptchaId) -> Result<(), Error> {
        self.send("captcha/skip", json!([captcha, "SINGLE"]))
    }

    pub fn accounts(&self) -> Result<Vec<Account>, Error> {
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

    pub fn add_account(&self, hoster: &str, username: &str, password: &str) -> Result<(), Error> {
        self.send("accountsV2/addAccount", json!([hoster, username, password]))
    }

    pub fn remove_accounts(&self, accounts: &[AccountId]) -> Result<(), Error> {
        self.send("accountsV2/removeAccounts", json!([accounts]))
    }

    pub fn enable_accounts(&self, accounts: &[AccountId]) -> Result<(), Error> {
        self.send("accountsV2/enableAccounts", json!([accounts]))
    }

    pub fn disable_accounts(&self, accounts: &[AccountId]) -> Result<(), Error> {
        self.send("accountsV2/disableAccounts", json!([accounts]))
    }

    pub fn refresh_accounts(&self, accounts: &[AccountId]) -> Result<(), Error> {
        self.send("accountsV2/refreshAccounts", json!([accounts, true]))
    }

    pub fn archives(&self) -> Result<Vec<ArchiveStatus>, Error> {
        self.call("extraction/getArchiveInfo", json!([[], []]))
    }

    pub fn start_extraction(&self, links: &[LinkId], packages: &[PackageId]) -> Result<(), Error> {
        self.send("extraction/startExtractionNow", json!([links, packages]))
    }

    pub fn set_archive_password(&self, archive: &ArchiveId, password: String) -> Result<(), Error> {
        let settings: Vec<ArchiveSettings> =
            self.call("extraction/getArchiveSettings", json!([[archive]]))?;
        let Some(settings) = settings.into_iter().next() else {
            return Err(Error::ArchiveNotFound(archive.to_string()));
        };
        let settings = settings.with_password(password);
        self.send("extraction/setArchiveSettings", json!([archive, settings]))
    }

    pub fn config_entries(&self, pattern: Option<&str>) -> Result<Vec<ConfigEntry>, Error> {
        let mut query = json!({ "values": true });
        if let Some(pattern) = pattern {
            query["pattern"] = json!(format!(".*{}.*", regex::escape(pattern)));
        }
        self.call("config/query", json!([query]))
    }

    pub fn config_get(&self, interface: &str, key: &str) -> Result<Value, Error> {
        self.call("config/get", json!([interface, null, key]))
    }

    pub fn config_set(&self, interface: &str, key: &str, value: &Value) -> Result<(), Error> {
        self.send("config/set", json!([interface, null, key, value]))
    }

    pub fn set_dupes_action(&self, action: DupesAction) -> Result<(), Error> {
        self.config_set(
            "org.jdownloader.gui.views.linkgrabber.addlinksdialog.LinkgrabberSettings",
            "DefaultOnAddedDupesLinksAction",
            &json!(action),
        )
    }

    pub fn plugins(&self) -> Result<Vec<Plugin>, Error> {
        self.call(
            "plugins/list",
            json!([{ "pattern": true, "version": true }]),
        )
    }

    pub fn plugin_settings(&self, hoster: &str) -> Result<Vec<PluginConfigEntry>, Error> {
        let query = json!({
            "pattern": format!(".*{}.*", regex::escape(hoster)),
            "values": true,
        });
        self.call("plugins/query", json!([query]))
    }

    fn url(&self, path: &str) -> Url {
        self.base.join(path.split('/'))
    }

    fn post(&self, path: &str, params: Value) -> Result<RequestBuilder, Error> {
        let request = self.client.post(self.url(path));
        Ok(json_body(request, &json!({ "params": params }))?)
    }

    fn send(&self, path: &str, params: Value) -> Result<(), Error> {
        self.fetch(self.post(path, params)?).map(drop)
    }

    fn call<T: DeserializeOwned>(&self, path: &str, params: Value) -> Result<T, Error> {
        let envelope: Envelope<T> = self.fetch(self.post(path, params)?)?.json()?;
        Ok(envelope.data)
    }

    /// Sends a request, failing with the server's error on a non-success status.
    fn fetch(&self, request: RequestBuilder) -> Result<Reply, Error> {
        let reply = bertbox_http::fetch(&self.client, request)?;
        if reply.status.is_success() {
            return Ok(reply);
        }
        let url = Box::new(reply.url);
        match serde_json::from_str::<ErrorResponse>(&reply.body) {
            Ok(error) => Err(Error::Server {
                url,
                src: error.src,
                kind: error.kind,
            }),
            Err(_) => Err(Error::Status {
                url,
                status: reply.status.as_u16(),
            }),
        }
    }
}
