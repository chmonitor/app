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

/// `[ui]` table — appearance preference (`system` / `light` / `dark`)
/// and the selected host id (`default` or a `[profiles.*]` key).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct UiSection {
    #[serde(default)]
    pub appearance: Option<String>,
    /// Active host: `"default"` for `[profile]`, or a `[profiles.<id>]` key.
    #[serde(default)]
    pub host: Option<String>,
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
    #[serde(default)]
    pub ui: UiSection,
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

/// Read `config.toml`, or an empty default when the file is missing/invalid.
pub fn load_config() -> ConfigFile {
    config_path()
        .as_deref()
        .map(load_config_from)
        .unwrap_or_default()
}

pub fn load_config_from(path: &Path) -> ConfigFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Write `config.toml`, creating the parent directory when needed.
pub fn save_config(cfg: &ConfigFile) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "no config directory on this platform".to_string())?;
    save_config_to(&path, cfg)
}

pub fn save_config_to(path: &Path, cfg: &ConfigFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
    }
    let out = toml::to_string_pretty(cfg).map_err(|e| format!("serialize failed: {e}"))?;
    std::fs::write(path, out).map_err(|e| format!("write failed: {e}"))
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

/// Id used for the unnamed `[profile]` host in the switcher.
pub const DEFAULT_HOST_ID: &str = "default";

/// One saved connection, for the host switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub id: String,
    pub label: String,
    pub profile: ProfileConfig,
}

/// Hostname (or fallback) shown in the switcher.
pub fn host_display(p: &ProfileConfig) -> String {
    match p.mode.as_deref() {
        Some("cloud") => host_from_url(p.base_url.as_deref()).unwrap_or_else(|| "cloud".into()),
        Some("clickhouse") => {
            host_from_url(p.url.as_deref()).unwrap_or_else(|| "clickhouse".into())
        }
        _ => "host".into(),
    }
}

pub fn host_label(id: &str, p: &ProfileConfig) -> String {
    if id != DEFAULT_HOST_ID {
        return id.to_string();
    }
    host_display(p)
}

/// Best-effort host[:port] from an HTTP(S) URL. `None` when empty/unusable.
pub fn host_from_url(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let rest = raw.split_once("://").map(|(_, r)| r).unwrap_or(raw);
    let hostport = rest.split('/').next().unwrap_or(rest);
    let host = hostport.split('@').next_back()?.split('?').next()?;
    let host = host.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Turn a Connect "Name" field into a host id. Empty/`default` → `[profile]`.
pub fn host_id_from_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned == DEFAULT_HOST_ID {
        DEFAULT_HOST_ID.into()
    } else {
        cleaned
    }
}

/// `[profile]` first, then named `[profiles.*]` in key order.
pub fn list_hosts(cfg: &ConfigFile) -> Vec<Host> {
    let mut out = Vec::new();
    if cfg.profile.mode.is_some() {
        out.push(Host {
            id: DEFAULT_HOST_ID.into(),
            label: host_label(DEFAULT_HOST_ID, &cfg.profile),
            profile: cfg.profile.clone(),
        });
    }
    for (id, p) in &cfg.profiles {
        if p.mode.is_none() || id.is_empty() || id == DEFAULT_HOST_ID {
            continue;
        }
        out.push(Host {
            id: id.clone(),
            label: host_label(id, p),
            profile: p.clone(),
        });
    }
    out
}

pub fn profile_for_host(cfg: &ConfigFile, id: &str) -> Option<ProfileConfig> {
    list_hosts(cfg)
        .into_iter()
        .find(|h| h.id == id)
        .map(|h| h.profile)
}

/// Env `CHM_PROFILE`, then `[ui].host`, then the first listed host.
pub fn active_host_id(cfg: &ConfigFile) -> Option<String> {
    let hosts = list_hosts(cfg);
    if hosts.is_empty() {
        return None;
    }
    let listed = |id: &str| hosts.iter().any(|h| h.id == id);
    if let Some(name) = profile_name_from_env()
        && listed(&name)
    {
        return Some(name);
    }
    if let Some(name) = cfg.ui.host.as_deref()
        && listed(name)
    {
        return Some(name.to_string());
    }
    Some(hosts[0].id.clone())
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
        cfg.ui.appearance = Some("dark".into());
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: ConfigFile = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn save_config_to_roundtrips_ui_and_channel() {
        let path = write_cfg("");
        let mut cfg = ConfigFile::default();
        cfg.ui.appearance = Some("light".into());
        cfg.profile.channel = Some("beta".into());
        cfg.telemetry.enabled = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert_eq!(back.ui.appearance.as_deref(), Some("light"));
        assert_eq!(back.profile.channel.as_deref(), Some("beta"));
        assert!(back.telemetry.enabled);
    }

    #[test]
    fn host_from_url_strips_scheme_and_path() {
        assert_eq!(
            host_from_url(Some("https://acme.dash.chmonitor.dev/api")),
            Some("acme.dash.chmonitor.dev".into())
        );
        assert_eq!(
            host_from_url(Some("http://localhost:8123")),
            Some("localhost:8123".into())
        );
        assert_eq!(host_from_url(Some("  ")), None);
        assert_eq!(host_from_url(None), None);
    }

    #[test]
    fn host_id_from_name_cleans_and_reserves_default() {
        assert_eq!(host_id_from_name(""), DEFAULT_HOST_ID);
        assert_eq!(host_id_from_name("default"), DEFAULT_HOST_ID);
        assert_eq!(host_id_from_name(" prod / eu "), "prod---eu");
        assert_eq!(host_id_from_name("work"), "work");
    }

    #[test]
    fn list_hosts_and_active_id() {
        let mut cfg = ConfigFile::default();
        cfg.profile.mode = Some("cloud".into());
        cfg.profile.base_url = Some("https://acme.dash.chmonitor.dev".into());
        cfg.profiles.insert(
            "work".into(),
            ProfileConfig {
                mode: Some("clickhouse".into()),
                url: Some("http://localhost:8123".into()),
                ..Default::default()
            },
        );
        let hosts = list_hosts(&cfg);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].id, DEFAULT_HOST_ID);
        assert_eq!(hosts[0].label, "acme.dash.chmonitor.dev");
        assert_eq!(hosts[1].id, "work");
        assert_eq!(active_host_id(&cfg).as_deref(), Some(DEFAULT_HOST_ID));
        cfg.ui.host = Some("work".into());
        assert_eq!(active_host_id(&cfg).as_deref(), Some("work"));
        cfg.ui.host = Some("missing".into());
        assert_eq!(active_host_id(&cfg).as_deref(), Some(DEFAULT_HOST_ID));
        assert_eq!(
            profile_for_host(&cfg, "work").unwrap().url.as_deref(),
            Some("http://localhost:8123")
        );
    }
}
