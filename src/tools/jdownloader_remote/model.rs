use std::collections::BTreeMap;
use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! numeric_id {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Clone, Copy, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(i64);

        impl FromStr for $name {
            type Err = ParseIntError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$name> for i64 {
            fn from(id: $name) -> i64 {
                id.0
            }
        }
    )*};
}

macro_rules! text_id {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Clone, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    )*};
}

numeric_id!(LinkId, PackageId, JobId, AccountId, CaptchaId);
text_id!(ArchiveId, VariantId);

#[derive(Clone, Copy, Serialize, ValueEnum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Priority {
    Highest,
    Higher,
    High,
    Default,
    Low,
    Lower,
    Lowest,
}

#[derive(Clone, Copy, Serialize, ValueEnum)]
pub enum CleanupAction {
    #[serde(rename = "DELETE_ALL")]
    All,
    #[serde(rename = "DELETE_DISABLED")]
    Disabled,
    #[serde(rename = "DELETE_FAILED")]
    Failed,
    #[serde(rename = "DELETE_FINISHED")]
    Finished,
    #[serde(rename = "DELETE_OFFLINE")]
    Offline,
    #[serde(rename = "DELETE_DUPE")]
    Dupe,
    #[serde(rename = "DELETE_MODE")]
    Mode,
}

#[derive(Clone, Copy, Serialize, ValueEnum)]
pub enum CleanupMode {
    #[serde(rename = "REMOVE_LINKS_AND_DELETE_FILES")]
    DeleteFiles,
    #[serde(rename = "REMOVE_LINKS_AND_RECYCLE_FILES")]
    RecycleFiles,
    #[serde(rename = "REMOVE_LINKS_ONLY")]
    LinksOnly,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CleanupSelection {
    Selected,
    All,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLinks {
    pub links: String,
    pub autostart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_password: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct LinkCollectingJob {
    pub id: JobId,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkVariant {
    pub id: Option<VariantId>,
    pub name: Option<String>,
    pub icon_key: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadLink {
    pub uuid: LinkId,
    pub name: String,
    #[serde(rename = "packageUUID")]
    pub package_uuid: PackageId,
    pub bytes_loaded: i64,
    pub bytes_total: i64,
    pub eta: Option<i64>,
    pub speed: i64,
    pub enabled: bool,
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub running: bool,
    pub skipped: Option<bool>,
    pub host: String,
    pub url: String,
    pub status: String,
    #[serde(rename = "jobUUID")]
    pub job_uuid: Option<JobId>,
    pub priority: Option<String>,
    pub added_date: i64,
    pub finished_date: Option<i64>,
    pub comment: Option<String>,
}

impl DownloadLink {
    pub fn percent(&self) -> i64 {
        percent(self.bytes_loaded, self.bytes_total)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePackage {
    pub uuid: PackageId,
    pub name: String,
    pub active_task: Option<String>,
    pub bytes_loaded: i64,
    pub bytes_total: i64,
    pub child_count: i64,
    pub comment: Option<String>,
    pub enabled: bool,
    pub eta: Option<i64>,
    pub finished: Option<bool>,
    pub hosts: Vec<String>,
    pub priority: Option<String>,
    pub running: Option<bool>,
    pub save_to: String,
    pub speed: Option<i64>,
    pub status: String,
}

impl FilePackage {
    pub fn percent(&self) -> i64 {
        percent(self.bytes_loaded, self.bytes_total)
    }
}

fn percent(loaded: i64, total: i64) -> i64 {
    if total <= 0 {
        return 0;
    }
    (loaded * 100).div_euclid(total)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawledLink {
    pub uuid: LinkId,
    pub name: String,
    #[serde(rename = "packageUUID")]
    pub package_uuid: PackageId,
    pub availability: String,
    pub bytes_total: Option<i64>,
    pub enabled: bool,
    pub host: String,
    #[serde(rename = "jobUUID")]
    pub job_uuid: Option<JobId>,
    pub url: String,
    pub variants: Option<bool>,
    pub variant: Option<LinkVariant>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptchaJob {
    pub id: CaptchaId,
    pub hoster: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub challenge_type: Option<String>,
    pub captcha_category: Option<String>,
    pub explain: Option<String>,
    pub link: Option<LinkId>,
    pub created: Option<i64>,
    pub timeout: Option<i64>,
    pub remaining: Option<i64>,
}

pub struct CaptchaImage {
    pub data: Vec<u8>,
    pub extension: String,
}

impl CaptchaImage {
    pub fn from_data_url(url: &str) -> Result<Self> {
        let (header, payload) = url.split_once(',').unwrap_or((url, ""));
        let mime = header
            .strip_prefix("data:")
            .unwrap_or(header)
            .split(';')
            .next()
            .unwrap_or_default();
        let extension = match mime.split_once('/') {
            Some((_, subtype)) if !subtype.is_empty() => subtype,
            _ => "png",
        };
        let data = STANDARD
            .decode(payload)
            .context("decoding the captcha image")?;
        Ok(Self {
            data,
            extension: extension.to_owned(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub uuid: AccountId,
    pub hostname: String,
    pub username: String,
    pub enabled: bool,
    pub valid: bool,
    pub error_type: Option<String>,
    pub error_string: Option<String>,
    pub traffic_left: Option<i64>,
    pub traffic_max: Option<i64>,
    pub valid_until: Option<i64>,
}

impl Account {
    pub fn error(&self) -> Option<&str> {
        match self.error_string.as_deref() {
            Some(error) if !error.is_empty() => Some(error),
            _ => self.error_type.as_deref(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveStatus {
    pub archive_id: ArchiveId,
    pub archive_name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub controller_id: Option<i64>,
    pub controller_status: Option<String>,
    pub states: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<ArchiveId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_extract: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passwords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_download_links_after_extraction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_files_after_extraction: Option<bool>,
}

impl ArchiveSettings {
    pub fn with_password(mut self, password: String) -> Self {
        let mut passwords = vec![password.clone()];
        passwords.extend(self.passwords.take().unwrap_or_default());
        self.passwords = Some(passwords);
        self.final_password = Some(password);
        self
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub interface_name: String,
    pub storage: Option<String>,
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub default_value: Value,
    pub docs: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigEntry {
    pub abstract_type: String,
    pub interface_name: String,
    pub key: String,
    pub storage: String,
    pub value: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub display_name: String,
    pub class_name: String,
    pub pattern: String,
    pub version: String,
    pub interface_name: Option<String>,
    pub key: Option<String>,
    #[serde(default)]
    pub value: Value,
}
