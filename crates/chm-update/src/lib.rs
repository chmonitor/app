//! Channel-aware update checker.
//!
//! Reads a static channel manifest (`{base}/{channel}.json`) over HTTPS and
//! compares it against the running version. This crate performs no phone-home:
//! the only outbound traffic is the explicit [`UpdateChecker::check`] call
//! against the configured manifest URL.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Base URL serving channel manifests in production.
pub const PRODUCTION_MANIFEST_BASE: &str = "https://updates.chmonitor.dev";

/// Environment variable overriding the manifest base in [`UpdateChecker::production`].
pub const MANIFEST_BASE_ENV: &str = "CHM_UPDATE_URL";

/// Release channel shown in manifests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("update manifest request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid update manifest: {0}")]
    BadManifest(String),
    #[error("invalid version in update manifest: {0}")]
    VersionParse(#[from] semver::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A release advertised on a channel manifest.
///
/// `sha256` is meant to be verified by the download/install step; this crate
/// never fetches the artifact itself.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseInfo {
    version: semver::Version,
    url: String,
    notes: String,
    sha256: Option<String>,
    date: Option<String>,
}

impl ReleaseInfo {
    pub fn version(&self) -> &semver::Version {
        &self.version
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn notes(&self) -> &str {
        &self.notes
    }

    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }

    pub fn date(&self) -> Option<&str> {
        self.date.as_deref()
    }
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    version: String,
    url: String,
    notes: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateChecker {
    manifest_base: String,
    client: reqwest::Client,
}

impl UpdateChecker {
    pub fn new(manifest_base: impl Into<String>) -> Self {
        Self {
            manifest_base: normalize_base(manifest_base.into()),
            client: reqwest::Client::new(),
        }
    }

    /// Checker pointed at the production manifest host. The `CHM_UPDATE_URL`
    /// environment variable overrides the base when set to a non-empty value.
    pub fn production() -> Self {
        let base = std::env::var(MANIFEST_BASE_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| PRODUCTION_MANIFEST_BASE.to_string());
        Self::new(base)
    }

    pub fn set_manifest_base(mut self, manifest_base: impl Into<String>) -> Self {
        self.manifest_base = normalize_base(manifest_base.into());
        self
    }

    pub fn manifest_base(&self) -> &str {
        &self.manifest_base
    }

    /// Fetches `{base}/{channel}.json` and returns the advertised release only
    /// when it is strictly newer than `current`.
    pub async fn check(
        &self,
        channel: Channel,
        current: &semver::Version,
    ) -> Result<Option<ReleaseInfo>> {
        let url = format!("{}/{}.json", self.manifest_base, channel.as_str());
        tracing::debug!(manifest_url = %url, %channel, "checking for updates");

        let body = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let raw: RawManifest =
            serde_json::from_str(&body).map_err(|e| Error::BadManifest(e.to_string()))?;
        let version: semver::Version = raw.version.parse()?;

        if version <= *current {
            return Ok(None);
        }

        Ok(Some(ReleaseInfo {
            version,
            url: raw.url,
            notes: raw.notes,
            sha256: raw.sha256,
            date: raw.date,
        }))
    }
}

fn normalize_base(base: String) -> String {
    base.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn manifest_json(version: &str) -> String {
        serde_json::json!({
            "version": version,
            "url": "https://downloads.chmonitor.dev/chmonitor.tar.zst",
            "notes": "bug fixes",
            "sha256": "deadbeef",
            "date": "2026-01-01"
        })
        .to_string()
    }

    async fn serve(path_name: &'static str, status: u16, body: String) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(path_name))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn channel_serde_and_display() {
        assert_eq!(Channel::default(), Channel::Stable);
        assert_eq!(serde_json::to_value(Channel::Beta).unwrap(), "beta");
        assert_eq!(
            serde_json::from_str::<Channel>("\"stable\"").unwrap(),
            Channel::Stable
        );
        assert_eq!(Channel::Beta.to_string(), "beta");
    }

    #[tokio::test]
    async fn newer_stable_release_is_returned() {
        let server = serve("/stable.json", 200, manifest_json("1.2.3")).await;
        let checker = UpdateChecker::new(server.uri());

        let release = checker
            .check(Channel::Stable, &semver::Version::new(1, 0, 0))
            .await
            .unwrap()
            .expect("1.2.3 > 1.0.0");

        assert_eq!(release.version().to_string(), "1.2.3");
        assert_eq!(
            release.url(),
            "https://downloads.chmonitor.dev/chmonitor.tar.zst"
        );
        assert_eq!(release.notes(), "bug fixes");
        assert_eq!(release.sha256(), Some("deadbeef"));
        assert_eq!(release.date(), Some("2026-01-01"));
    }

    #[tokio::test]
    async fn equal_version_returns_none() {
        let server = serve("/stable.json", 200, manifest_json("1.2.3")).await;
        let checker = UpdateChecker::new(server.uri());

        let update = checker
            .check(Channel::Stable, &semver::Version::new(1, 2, 3))
            .await
            .unwrap();

        assert!(update.is_none());
    }

    #[tokio::test]
    async fn older_version_returns_none() {
        let server = serve("/stable.json", 200, manifest_json("1.2.3")).await;
        let checker = UpdateChecker::new(server.uri());

        let update = checker
            .check(Channel::Stable, &semver::Version::new(2, 0, 0))
            .await
            .unwrap();

        assert!(update.is_none());
    }

    #[tokio::test]
    async fn beta_channel_requests_beta_manifest() {
        let server = serve("/beta.json", 200, manifest_json("2.0.0-beta.1")).await;
        let checker = UpdateChecker::new(server.uri());

        let release = checker
            .check(Channel::Beta, &semver::Version::new(1, 0, 0))
            .await
            .unwrap()
            .expect("prerelease bump counts as newer");

        assert_eq!(release.version().to_string(), "2.0.0-beta.1");

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/beta.json");
    }

    #[tokio::test]
    async fn malformed_manifest_is_bad_manifest_error() {
        let server = serve("/stable.json", 200, "{not json".to_string()).await;
        let checker = UpdateChecker::new(server.uri());

        let err = checker
            .check(Channel::Stable, &semver::Version::new(1, 0, 0))
            .await
            .unwrap_err();

        assert!(matches!(err, Error::BadManifest(_)), "{err:?}");
    }

    #[tokio::test]
    async fn unparsable_version_is_version_parse_error() {
        let server = serve("/stable.json", 200, manifest_json("not-a-semver")).await;
        let checker = UpdateChecker::new(server.uri());

        let err = checker
            .check(Channel::Stable, &semver::Version::new(1, 0, 0))
            .await
            .unwrap_err();

        assert!(matches!(err, Error::VersionParse(_)), "{err:?}");
    }

    #[tokio::test]
    async fn server_error_is_http_error() {
        let server = serve("/stable.json", 500, "boom".to_string()).await;
        let checker = UpdateChecker::new(server.uri());

        let err = checker
            .check(Channel::Stable, &semver::Version::new(1, 0, 0))
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Http(_)), "{err:?}");
    }

    #[test]
    fn manifest_base_normalizes_trailing_slashes() {
        let checker = UpdateChecker::new("https://updates.example.com/");
        assert_eq!(checker.manifest_base(), "https://updates.example.com");
        assert_eq!(
            UpdateChecker::new("x")
                .set_manifest_base("https://y.io///")
                .manifest_base(),
            "https://y.io"
        );
    }
}
