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
    #[error("update download checksum mismatch: expected {expected}, got {got}")]
    Checksum { expected: String, got: String },
    #[error("update download io failed: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A release advertised on a channel manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseInfo {
    version: semver::Version,
    url: String,
    notes: String,
    sha256: Option<String>,
    date: Option<String>,
    target: Option<String>,
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

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
}

/// Rustc target triple for this binary, used to pick a row from a
/// multi-target channel manifest.
pub fn current_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        _ => "unknown",
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
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ManifestBody {
    One(RawManifest),
    Many(Vec<RawManifest>),
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

        let body: ManifestBody =
            serde_json::from_str(&body).map_err(|e| Error::BadManifest(e.to_string()))?;
        let raw = pick_manifest(body, current_target())
            .ok_or_else(|| Error::BadManifest("empty update manifest".into()))?;
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
            target: raw.target,
        }))
    }

    /// Downloads `release.url` to `dest` and verifies `sha256` when present.
    pub async fn download(&self, release: &ReleaseInfo, dest: &std::path::Path) -> Result<()> {
        let bytes = self
            .client
            .get(release.url())
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        if let Some(expected) = release.sha256() {
            let got = sha256_hex(&bytes);
            if !got.eq_ignore_ascii_case(expected) {
                return Err(Error::Checksum {
                    expected: expected.to_string(),
                    got,
                });
            }
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, bytes)?;
        Ok(())
    }
}

fn pick_manifest(body: ManifestBody, target: &str) -> Option<RawManifest> {
    match body {
        ManifestBody::One(one) => Some(one),
        ManifestBody::Many(rows) if rows.is_empty() => None,
        ManifestBody::Many(mut rows) => {
            if let Some(ix) = rows
                .iter()
                .position(|r| r.target.as_deref() == Some(target))
            {
                Some(rows.swap_remove(ix))
            } else {
                Some(rows.swap_remove(0))
            }
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
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

    #[tokio::test]
    async fn multi_target_manifest_picks_matching_row() {
        let body = serde_json::json!([
            {
                "version": "1.2.3",
                "url": "https://dl.example/linux.tar.gz",
                "notes": "linux",
                "target": "x86_64-unknown-linux-gnu"
            },
            {
                "version": "1.2.3",
                "url": "https://dl.example/mac.zip",
                "notes": "mac",
                "sha256": "abc",
                "target": current_target()
            }
        ])
        .to_string();
        let server = serve("/stable.json", 200, body).await;
        let checker = UpdateChecker::new(server.uri());
        let release = checker
            .check(Channel::Stable, &semver::Version::new(1, 0, 0))
            .await
            .unwrap()
            .expect("newer");
        let target = current_target();
        let (expected_url, expected_sha) = if target == "x86_64-unknown-linux-gnu" {
            ("https://dl.example/linux.tar.gz", None)
        } else {
            ("https://dl.example/mac.zip", Some("abc"))
        };
        assert_eq!(release.url(), expected_url);
        assert_eq!(release.target(), Some(target));
        assert_eq!(release.sha256(), expected_sha);
    }

    #[tokio::test]
    async fn download_writes_file_and_checks_sha256() {
        let payload = b"chmonitor-update-bytes";
        let digest = sha256_hex(payload);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pkg.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.as_slice()))
            .mount(&server)
            .await;
        let checker = UpdateChecker::new(server.uri());
        let release = ReleaseInfo {
            version: semver::Version::new(1, 2, 3),
            url: format!("{}/pkg.zip", server.uri()),
            notes: String::new(),
            sha256: Some(digest),
            date: None,
            target: None,
        };
        let dest = std::env::temp_dir().join(format!(
            "chm-update-dl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        checker.download(&release, &dest).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn download_rejects_bad_checksum() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pkg.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"nope"))
            .mount(&server)
            .await;
        let checker = UpdateChecker::new(server.uri());
        let release = ReleaseInfo {
            version: semver::Version::new(1, 2, 3),
            url: format!("{}/pkg.zip", server.uri()),
            notes: String::new(),
            sha256: Some("deadbeef".into()),
            date: None,
            target: None,
        };
        let dest = std::env::temp_dir().join("chm-update-bad-sha");
        let err = checker.download(&release, &dest).await.unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }), "{err:?}");
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
