//! Connection profile, config.toml, CLI flags.
//!
//! Kept out of the GPUI shell so the parse/load path can be unit-tested
//! without opening a window.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chm_clickhouse::ClickHouseClient;
use chm_cloud_api::CloudClient;
use chm_core::DataSource;

/// Saved connection profile (`[profile]` table, or `[profiles.<name>]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct ProfileConfig {
    /// "cloud" | "clickhouse"
    pub mode: Option<String>,
    /// Cloud mode: API base URL.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Cloud mode: API key.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Direct mode: ClickHouse HTTP endpoint.
    #[serde(default)]
    pub url: Option<String>,
    /// Direct mode: user name.
    #[serde(default)]
    pub user: Option<String>,
    /// Direct mode: password.
    #[serde(default)]
    pub password: Option<String>,
    /// Release channel for the update check: "stable" | "beta".
    #[serde(default)]
    pub channel: Option<String>,
}

/// `[telemetry]` table — opt-in, default disabled.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct TelemetrySection {
    #[serde(default)]
    pub enabled: bool,
}

/// Whole `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub profile: ProfileConfig,
    /// Named profiles selected with `CHM_PROFILE=<name>`.
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
    #[serde(default)]
    pub telemetry: TelemetrySection,
}

/// `<config_dir>/chmonitor/config.toml`, or `CHM_CONFIG` when set.
pub fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CHM_CONFIG").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    dirs::config_dir().map(|d| d.join("chmonitor").join("config.toml"))
}

/// `CHM_PROFILE` value when non-empty.
pub fn profile_name_from_env() -> Option<String> {
    std::env::var("CHM_PROFILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Read the saved profile, if a well-formed file exists. Any failure means
/// "no profile" and the app shows the Connect screen.
pub fn load_profile() -> Option<ProfileConfig> {
    load_profile_from(config_path()?.as_path(), profile_name_from_env().as_deref())
}

/// Load `[profile]` or `[profiles.<named>]` from an explicit path.
pub fn load_profile_from(path: &Path, named: Option<&str>) -> Option<ProfileConfig> {
    let text = std::fs::read_to_string(path).ok()?;
    let cfg: ConfigFile = toml::from_str(&text).ok()?;
    match named.filter(|n| !n.is_empty()) {
        Some(name) => cfg.profiles.get(name).cloned().filter(|p| p.mode.is_some()),
        None => {
            cfg.profile.mode.as_ref()?;
            Some(cfg.profile)
        }
    }
}

/// Build the boxed data source behind [`chm_core::DataSource`] for a saved
/// profile. `None` when required fields are missing.
pub fn source_from_profile(p: &ProfileConfig) -> Option<Box<dyn DataSource>> {
    match p.mode.as_deref()? {
        "cloud" => Some(Box::new(CloudClient::new(
            p.base_url.clone()?,
            p.api_key.clone(),
        ))),
        "clickhouse" => Some(Box::new(ClickHouseClient::new(
            p.url.clone()?,
            p.user.clone().unwrap_or_else(|| "default".into()),
            p.password.clone(),
        ))),
        _ => None,
    }
}

/// Flags parsed from argv. Stored process-wide so `Shell::new` can read them
/// without threading gpui constructors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cli {
    /// Force the Connect page even when a profile exists.
    pub connect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    Help,
    Version,
    Unknown(String),
}

pub const HELP: &str = "\
chmonitor desktop — ClickHouse monitoring

Usage:
  chm-app [--connect]

Options:
  --connect     Open the Connect screen
  -h, --help    Show this help
  -V, --version Print version

Environment:
  CHM_SMOKE=1           Use fixture data (no network)
  CHM_PROFILE=<name>    Load [profiles.<name>] from config.toml
  CHM_CONFIG=<path>     Override config.toml path
  CHM_UPDATE_URL=<url>  Override update manifest base
";

impl Cli {
    pub fn parse<I, S>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut cli = Cli::default();
        for arg in args {
            match arg.as_ref() {
                "--connect" => cli.connect = true,
                "-h" | "--help" => return Err(CliError::Help),
                "-V" | "--version" => return Err(CliError::Version),
                other => return Err(CliError::Unknown(other.to_string())),
            }
        }
        Ok(cli)
    }
}

static CLI: OnceLock<Cli> = OnceLock::new();

pub fn install_cli(cli: Cli) {
    let _ = CLI.set(cli);
}

pub fn cli() -> Cli {
    CLI.get().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_cfg(body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "chm-config-tests-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!("{n}.toml"));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn cli_connect_and_help() {
        assert_eq!(Cli::parse(["--connect"]).unwrap(), Cli { connect: true });
        assert_eq!(Cli::parse(Vec::<&str>::new()).unwrap(), Cli::default());
        assert_eq!(Cli::parse(["--help"]).unwrap_err(), CliError::Help);
        assert_eq!(Cli::parse(["-V"]).unwrap_err(), CliError::Version);
        assert!(matches!(
            Cli::parse(["--nope"]),
            Err(CliError::Unknown(s)) if s == "--nope"
        ));
    }

    #[test]
    fn default_profile_requires_mode() {
        let path = write_cfg("[profile]\nbase_url = \"https://x\"\n");
        assert!(load_profile_from(&path, None).is_none());
    }

    #[test]
    fn default_profile_loads_cloud() {
        let path = write_cfg(
            "[profile]\nmode = \"cloud\"\nbase_url = \"https://acme.dash.chmonitor.dev\"\napi_key = \"k\"\n",
        );
        let p = load_profile_from(&path, None).unwrap();
        assert_eq!(p.mode.as_deref(), Some("cloud"));
        assert_eq!(
            p.base_url.as_deref(),
            Some("https://acme.dash.chmonitor.dev")
        );
        let src = source_from_profile(&p).unwrap();
        assert_eq!(src.label(), "cloud: https://acme.dash.chmonitor.dev");
    }

    #[test]
    fn named_profile_selected_over_default() {
        let path = write_cfg(
            r#"
[profile]
mode = "cloud"
base_url = "https://default.example"

[profiles.work]
mode = "clickhouse"
url = "http://localhost:8123"
user = "alice"
"#,
        );
        let def = load_profile_from(&path, None).unwrap();
        assert_eq!(def.mode.as_deref(), Some("cloud"));
        let work = load_profile_from(&path, Some("work")).unwrap();
        assert_eq!(work.mode.as_deref(), Some("clickhouse"));
        assert_eq!(work.user.as_deref(), Some("alice"));
        let src = source_from_profile(&work).unwrap();
        assert_eq!(src.label(), "clickhouse: http://localhost:8123");
        assert!(load_profile_from(&path, Some("missing")).is_none());
    }

    #[test]
    fn source_rejects_incomplete_and_unknown_modes() {
        assert!(source_from_profile(&ProfileConfig::default()).is_none());
        assert!(
            source_from_profile(&ProfileConfig {
                mode: Some("cloud".into()),
                ..Default::default()
            })
            .is_none()
        );
        assert!(
            source_from_profile(&ProfileConfig {
                mode: Some("clickhouse".into()),
                ..Default::default()
            })
            .is_none()
        );
        assert!(
            source_from_profile(&ProfileConfig {
                mode: Some("ftp".into()),
                url: Some("http://x".into()),
                ..Default::default()
            })
            .is_none()
        );
    }

    #[test]
    fn clickhouse_defaults_user_to_default() {
        let src = source_from_profile(&ProfileConfig {
            mode: Some("clickhouse".into()),
            url: Some("http://ch:8123".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(src.label(), "clickhouse: http://ch:8123");
    }

    #[test]
    fn save_roundtrip_keeps_named_profiles_and_telemetry() {
        let mut cfg = ConfigFile::default();
        cfg.profile.mode = Some("cloud".into());
        cfg.profile.base_url = Some("https://a".into());
        cfg.profiles.insert(
            "work".into(),
            ProfileConfig {
                mode: Some("clickhouse".into()),
                url: Some("http://localhost:8123".into()),
                ..Default::default()
            },
        );
        cfg.telemetry.enabled = true;
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: ConfigFile = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }
}
