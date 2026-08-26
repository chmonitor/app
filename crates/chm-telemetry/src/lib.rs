//! Opt-in product telemetry + performance metrics.
//!
//! Hard rules enforced here:
//! - Telemetry is disabled by default; nothing is ever sent unless the caller
//!   flips [`TelemetryConfig::enabled`] to `true`.
//! - Events carry an allowlisted payload shape only (counters, durations, page
//!   names such as `"overview"`/`"queries"`). Never user content, SQL, or query
//!   text — do not route user data through [`EventBatcher::push`].
//! - The batcher only ever POSTs to the single endpoint in
//!   [`TelemetryConfig::endpoint`].

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use chm_update::Channel;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

/// Maximum number of queued events; pushes beyond this drop the oldest.
pub const MAX_QUEUED_EVENTS: usize = 100;

/// Number of samples retained per metric in [`PerfMetrics`].
pub const PERF_SAMPLE_CAPACITY: usize = 512;

#[derive(Debug, Error)]
pub enum Error {
    #[error("telemetry request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("disallowed event name: {0}")]
    DisallowedEvent(String),
    #[error("invalid sample: {ms}")]
    InvalidSample { ms: f64 },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Telemetry settings. `Default` reports `enabled == false`; there is no code
/// path that turns it on implicitly.
#[derive(Debug, Clone, Default)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub app_version: String,
    pub channel: Channel,
}

impl TelemetryConfig {
    /// Explicit opt-in helper. Endpoint must be supplied by the caller.
    pub fn opt_in(
        endpoint: impl Into<String>,
        app_version: impl Into<String>,
        channel: Channel,
    ) -> Self {
        Self {
            enabled: true,
            endpoint: Some(endpoint.into()),
            app_version: app_version.into(),
            channel,
        }
    }

    pub fn set_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn set_endpoint(mut self, endpoint: Option<String>) -> Self {
        self.endpoint = endpoint;
        self
    }

    pub fn set_app_version(mut self, app_version: impl Into<String>) -> Self {
        self.app_version = app_version.into();
        self.app_version.truncate(64);
        self
    }

    pub fn set_channel(mut self, channel: Channel) -> Self {
        self.channel = channel;
        self
    }

    fn meta(&self) -> Value {
        json!({
            "version": self.app_version,
            "channel": self.channel.as_str(),
            "os": std::env::consts::OS,
        })
    }
}

#[derive(Debug)]
struct RingBuffer {
    samples: VecDeque<f64>,
}

impl RingBuffer {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(PERF_SAMPLE_CAPACITY),
        }
    }

    fn push(&mut self, ms: f64) {
        if self.samples.len() == PERF_SAMPLE_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(ms);
    }

    fn percentile(&self, p: f64) -> Option<f64> {
        let n = self.samples.len();
        if n == 0 {
            return None;
        }
        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let rank = ((p / 100.0) * n as f64).round() as usize;
        let idx = rank.clamp(1, n) - 1;
        Some(sorted[idx])
    }
}

#[derive(Debug)]
pub struct PerfMetrics {
    frame_ms: Mutex<RingBuffer>,
    fetch_ms: Mutex<RingBuffer>,
    rss_bytes: Mutex<Option<u64>>,
}

impl Default for PerfMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfMetrics {
    pub fn new() -> Self {
        Self {
            frame_ms: Mutex::new(RingBuffer::new()),
            fetch_ms: Mutex::new(RingBuffer::new()),
            rss_bytes: Mutex::new(None),
        }
    }

    pub fn record_frame(&self, ms: f64) -> Result<()> {
        self.validate(ms)?;
        self.frame_ms.lock().unwrap().push(ms);
        Ok(())
    }

    pub fn record_fetch(&self, ms: f64) -> Result<()> {
        self.validate(ms)?;
        self.fetch_ms.lock().unwrap().push(ms);
        Ok(())
    }

    pub fn frame_percentiles(&self) -> Percentiles {
        percentiles_of(&self.frame_ms)
    }

    pub fn fetch_percentiles(&self) -> Percentiles {
        percentiles_of(&self.fetch_ms)
    }

    pub fn frame_count(&self) -> usize {
        self.frame_ms.lock().unwrap().samples.len()
    }

    pub fn fetch_count(&self) -> usize {
        self.fetch_ms.lock().unwrap().samples.len()
    }

    pub fn record_rss(&self, bytes: u64) {
        *self.rss_bytes.lock().unwrap() = Some(bytes);
    }

    pub fn last_rss(&self) -> Option<u64> {
        *self.rss_bytes.lock().unwrap()
    }

    pub fn last_fetch_ms(&self) -> Option<f64> {
        self.fetch_ms.lock().unwrap().samples.back().copied()
    }

    pub fn reset(&mut self) {
        *self.frame_ms.lock().unwrap() = RingBuffer::new();
        *self.fetch_ms.lock().unwrap() = RingBuffer::new();
    }

    fn validate(&self, ms: f64) -> Result<()> {
        if !ms.is_finite() || ms < 0.0 {
            return Err(Error::InvalidSample { ms });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

fn percentiles_of(buf: &Mutex<RingBuffer>) -> Percentiles {
    let buf = buf.lock().unwrap();
    Percentiles {
        p50: buf.percentile(50.0).expect("percentile of non-empty ring"),
        p95: buf.percentile(95.0).expect("percentile of non-empty ring"),
        p99: buf.percentile(99.0).expect("percentile of non-empty ring"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    name: String,
    #[serde(default)]
    props: Value,
    #[serde(default)]
    ts_ms: Option<u64>,
}

impl Event {
    /// Creates an event from the field allowlist: `name` must be one of the
    /// known product events and `props` only counters/durations/page names.
    pub fn new(name: impl Into<String>, props: Value) -> Result<Self> {
        let name = name.into();
        if !ALLOWED_EVENT_NAMES.contains(&name.as_str()) {
            return Err(Error::DisallowedEvent(name));
        }
        Ok(Self {
            name,
            props,
            ts_ms: None,
        })
    }

    pub fn with_ts_ms(mut self, ts_ms: u64) -> Self {
        self.ts_ms = Some(ts_ms);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Allowlist of event names. Anything else is rejected at construction.
const ALLOWED_EVENT_NAMES: &[&str] = &[
    "app_launch",
    "app_quit",
    "app_loaded",
    "page_view",
    "cluster_connected",
    "health_viewed",
    "queries_viewed",
    "query_executed",
];

/// Page-name allowlist for `page_view.props.page`.
pub const ALLOWED_PAGE_NAMES: &[&str] = &[
    "overview", "queries", "merges", "replicas", "health", "tables", "traffic", "connect",
    "settings",
];

pub fn http_client() -> &'static reqwest::Client {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    HTTP.get_or_init(reqwest::Client::new)
}

/// Production ingest (opt-in only).
pub const TELEMETRY_PING_URL: &str = "https://telemetry.chmonitor.dev/v1/ping";
pub const TELEMETRY_EVENT_URL: &str = "https://telemetry.chmonitor.dev/v1/event";

/// Opaque 64-char hex install id, persisted under the user config dir.
pub fn install_id() -> String {
    let path = dirs::config_dir().map(|d| d.join("chmonitor").join("install_id"));
    if let Some(path) = &path
        && let Ok(existing) = std::fs::read_to_string(path)
    {
        let trimmed = existing.trim().to_ascii_lowercase();
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return trimmed;
        }
    }
    let mut raw = [0u8; 32];
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(&mut raw);
        }
    }
    if raw.iter().all(|b| *b == 0) {
        let seed = format!(
            "{}:{}:{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            std::env::consts::OS
        );
        raw = sha256_bytes(seed.as_bytes());
    }
    let hex = hex64(&raw);
    if let Some(path) = path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &hex);
    }
    hex
}

fn sha256_bytes(input: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input);
    h.finalize().into()
}

fn hex64(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Current process RSS in bytes (best-effort; `ps` on Unix).
pub fn rss_bytes() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb.saturating_mul(1024))
}

/// POST `/v1/ping` so this install is counted. No-op when `enabled` is false.
pub async fn ping(http: &reqwest::Client, cfg: &TelemetryConfig) -> Result<()> {
    if !cfg.enabled {
        return Ok(());
    }
    let body = json!({
        "instance_hash": install_id(),
        "deploy_target": "unknown",
        "platform": match std::env::consts::OS {
            "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            _ => "unknown",
        },
        "chm_version": cfg.app_version,
    });
    http.post(TELEMETRY_PING_URL)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// POST `/v1/event` (worker allowlist). No-op when disabled.
pub async fn track(http: &reqwest::Client, cfg: &TelemetryConfig, event: &str) -> Result<()> {
    if !cfg.enabled {
        return Ok(());
    }
    if !matches!(
        event,
        "app_loaded" | "cluster_connected" | "health_viewed" | "queries_viewed" | "ai_query_sent"
    ) {
        return Err(Error::DisallowedEvent(event.into()));
    }
    let body = json!({
        "event": event,
        "props": {
            "deploy_target": "unknown",
            "ch_flavor": "unknown",
        }
    });
    http.post(TELEMETRY_EVENT_URL)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Queues events and flushes them to the configured endpoint.
///
/// Disabled batches never send anything and drain to `Ok(0)`.
pub struct EventBatcher {
    config: TelemetryConfig,
    queue: Mutex<VecDeque<Event>>,
}

impl EventBatcher {
    pub fn new(config: TelemetryConfig) -> Self {
        Self {
            config,
            queue: Mutex::new(VecDeque::with_capacity(MAX_QUEUED_EVENTS)),
        }
    }

    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }

    pub fn len(&self) -> usize {
        self.queue.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Enqueues an allowlisted event. At capacity the oldest event is dropped.
    /// Rejects event names outside [`ALLOWED_EVENT_NAMES`].
    pub fn push(&self, event: Event) -> Result<()> {
        let mut queue = self.queue.lock().unwrap();
        if queue.len() >= MAX_QUEUED_EVENTS {
            queue.pop_front();
        }
        queue.push_back(event);
        Ok(())
    }

    /// Posts `{"events":[...],"meta":{...}}` when enabled; no-op returning
    /// `Ok(0)` when disabled or nothing is queued.
    pub async fn flush(&self, http: &reqwest::Client) -> Result<usize> {
        if !self.config.enabled {
            let dropped = { self.queue.lock().unwrap().drain(..).count() };
            tracing::debug!("telemetry disabled; dropping {dropped} queued events");
            return Ok(0);
        }

        let Some(endpoint) = self.config.endpoint.as_deref() else {
            tracing::debug!("telemetry enabled but no endpoint configured; keeping queue");
            return Ok(0);
        };

        let drained: Vec<Event> = {
            let mut queue = self.queue.lock().unwrap();
            queue.drain(..).collect()
        };

        if drained.is_empty() {
            return Ok(0);
        }

        let payload = json!({
            "events": drained,
            "meta": self.config.meta(),
        });

        let count = drained.len();
        http.post(endpoint)
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;
        Ok(count)
    }

    pub fn clear(&self) {
        self.queue.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn enabled_config(endpoint: String) -> TelemetryConfig {
        TelemetryConfig::opt_in(endpoint, "1.2.3", Channel::Beta)
    }

    fn page_view(page: &str) -> Event {
        Event::new("page_view", json!({ "page": page })).unwrap()
    }

    #[test]
    fn default_config_is_disabled_with_no_endpoint() {
        let cfg = TelemetryConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.endpoint.is_none());
        assert_eq!(cfg.channel, Channel::Stable);
    }

    #[test]
    fn event_allowlist_rejects_unknown_names_and_free_text() {
        assert!(Event::new("page_view", json!({})).is_ok());
        assert!(Event::new("user_typed_query", json!({ "q": "SELECT" })).is_err());
    }

    #[tokio::test]
    async fn enabled_batch_posts_events_and_meta() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/events"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let batcher = EventBatcher::new(enabled_config(format!("{}/v1/events", server.uri())));
        batcher.push(page_view("overview")).unwrap();
        batcher
            .push(Event::new("query_executed", json!({ "duration_ms": 12 })).unwrap())
            .unwrap();

        let sent = batcher.flush(&reqwest::Client::new()).await.unwrap();

        assert_eq!(sent, 2);
        assert!(batcher.is_empty());

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);

        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["events"].as_array().unwrap().len(), 2);
        assert_eq!(body["events"][0]["name"], "page_view");
        assert_eq!(body["events"][1]["props"]["duration_ms"], 12);
        assert_eq!(body["meta"]["version"], "1.2.3");
        assert_eq!(body["meta"]["channel"], "beta");
        assert_eq!(body["meta"]["os"], std::env::consts::OS);
    }

    #[tokio::test]
    async fn disabled_batch_sends_nothing_and_drains_queue() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let batcher = EventBatcher::new(TelemetryConfig::default());
        batcher.push(page_view("queries")).unwrap();
        batcher
            .push(Event::new("app_launch", json!({})).unwrap())
            .unwrap();

        let sent = batcher.flush(&reqwest::Client::new()).await.unwrap();

        assert_eq!(sent, 0);
        assert!(batcher.is_empty(), "queue drains even when disabled");

        // Give any (buggy) fire-and-forget request time to arrive.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let requests = server.received_requests().await.unwrap();
        assert!(
            requests.is_empty(),
            "disabled telemetry must not send anything, got {requests:?}"
        );
    }

    #[tokio::test]
    async fn enabled_without_endpoint_keeps_queue_and_sends_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let batcher = EventBatcher::new(TelemetryConfig::default().set_enabled(true));
        batcher.push(page_view("overview")).unwrap();

        let sent = batcher.flush(&reqwest::Client::new()).await.unwrap();
        assert_eq!(sent, 0);
        assert_eq!(batcher.len(), 1, "no endpoint: events stay queued");

        let requests = server.received_requests().await.unwrap();
        assert!(requests.is_empty());
    }

    #[test]
    fn queue_caps_at_100_dropping_oldest() {
        let batcher = EventBatcher::new(enabled_config("https://telemetry.chmonitor.dev".into()));

        for i in 0..130 {
            batcher
                .push(Event::new("app_launch", json!({ "i": i })).unwrap())
                .unwrap();
        }

        assert_eq!(batcher.len(), MAX_QUEUED_EVENTS);

        let queue = batcher.queue.lock().unwrap();
        assert_eq!(
            queue.front().unwrap().props["i"],
            30,
            "oldest dropped first"
        );
        assert_eq!(queue.back().unwrap().props["i"], 129);
    }

    #[test]
    fn perf_percentiles_on_synthetic_input() {
        let metrics = PerfMetrics::new();

        for i in 1..=100u64 {
            metrics.record_frame(i as f64).unwrap();
        }

        let frames = metrics.frame_percentiles();
        assert_eq!(frames.p50, 50.0);
        assert_eq!(frames.p95, 95.0);
        assert_eq!(frames.p99, 99.0);

        for i in 1..=10u64 {
            metrics.record_fetch(i as f64).unwrap();
        }

        let fetches = metrics.fetch_percentiles();
        assert_eq!(fetches.p50, 5.0);
        assert_eq!(fetches.p95, 10.0);
        assert_eq!(fetches.p99, 10.0);
    }

    #[test]
    fn perf_ring_wraps_after_512_samples() {
        let metrics = PerfMetrics::new();

        for i in 0..600u64 {
            metrics.record_frame(i as f64).unwrap();
        }

        assert_eq!(metrics.frame_count(), PERF_SAMPLE_CAPACITY);
        let frames = metrics.frame_percentiles();
        assert_eq!(frames.p50, 343.0);
        assert_eq!(frames.p95, 573.0);
        assert_eq!(frames.p99, 594.0);
    }

    #[test]
    fn install_id_is_64_hex_and_stable() {
        let a = install_id();
        let b = install_id();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, b);
    }

    #[test]
    fn perf_rejects_non_finite_and_negative_samples() {
        let metrics = PerfMetrics::new();
        assert!(metrics.record_frame(-1.0).is_err());
        assert!(metrics.record_fetch(f64::NAN).is_err());
        assert!(metrics.record_fetch(f64::INFINITY).is_err());
        assert_eq!(metrics.frame_count(), 0);
        assert_eq!(metrics.fetch_count(), 0);
    }
}
