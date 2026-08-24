//! Mode 1 — REST client for a chmonitor dashboard deployment (cloud or
//! self-hosted worker).
//!
//! The dashboard's `/api/*` routes (`apps/dashboard/src/routes/api/*`) are
//! ad-hoc per page with NO stable documented contract, so this client is
//! deliberately pragmatic:
//!
//! * well-known endpoints are probed where they exist (`/api/healthz`,
//!   `/api/overview`, …);
//! * every payload is mapped tolerantly — missing/renamed fields fall back
//!   through alias-key lists into domain-type defaults instead of erroring;
//! * only transport failure, non-2xx status, or an unusable top-level shape
//!   becomes a [`DataSourceError`] (401/403 → `Auth`, rest → `Query`);
//! * [`CloudClient::get_raw`] is a public passthrough so the UI can call any
//!   ad-hoc dashboard route directly without extending this crate.

use async_trait::async_trait;
use chm_core::{
    DataSource, DataSourceError, Health, MergeRow, Overview, QueryRow, ReplicaRow, Result,
    SeriesPoint, TableStat, TimeRange, TrafficSeries,
};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::Value;

/// REST client for a chmonitor dashboard deployment; implements
/// [`chm_core::DataSource`].
#[derive(Debug, Clone)]
pub struct CloudClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl CloudClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            // Normalize so `https://host/` and `https://host` behave alike.
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            http: reqwest::Client::new(),
        }
    }

    /// Public passthrough: GET `{base}/api/{path_and_query}`, JSON-parsed.
    /// Accepts `"route"` or `"route?a=b"`; lets the UI reach ad-hoc
    /// dashboard endpoints without new typed client code.
    pub async fn get_raw(&self, path_and_query: &str) -> Result<Value> {
        let resp = self.request(path_and_query).await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(status_err(status, path_and_query));
        }
        resp.json().await.map_err(|e| DataSourceError::Query {
            message: format!("bad JSON from {path_and_query}: {e}"),
        })
    }

    /// Shared GET with bearer auth; network failure maps to `Connection`.
    async fn request(&self, path_and_query: &str) -> Result<reqwest::Response> {
        let url = format!(
            "{}/api/{}",
            self.base_url,
            path_and_query.trim_start_matches('/')
        );
        let mut req = self.http.get(&url);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req.send().await.map_err(|e| DataSourceError::Connection {
            message: e.to_string(),
        })
    }

    /// Generic list fetch: GET `{path}` and map whatever row array comes back.
    async fn row_list<T>(&self, path: &str, map: fn(&Value) -> Option<T>) -> Result<Vec<T>> {
        let v = self.get_raw(path).await?;
        let rows = rows_of(&v).ok_or_else(|| DataSourceError::Query {
            message: format!("unusable shape from /api/{path}"),
        })?;
        Ok(rows.into_iter().filter_map(map).collect())
    }
}

/// 401/403 mean rejected credentials (`Auth`); any other non-2xx is `Query`.
fn status_err(status: reqwest::StatusCode, path: &str) -> DataSourceError {
    let code = status.as_u16();
    if code == 401 || code == 403 {
        DataSourceError::Auth {
            message: format!("HTTP {code} on {path}"),
        }
    } else {
        DataSourceError::Query {
            message: format!("HTTP {code} on {path}"),
        }
    }
}

/// First non-empty string among alias keys; numbers are stringified.
fn first_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => return s.clone(),
            Some(Value::Number(n)) => return n.to_string(),
            _ => {}
        }
    }
    String::new()
}

/// First coercible number among alias keys (JSON number or numeric string).
fn first_num(v: &Value, keys: &[&str]) -> f64 {
    for k in keys {
        match v.get(*k) {
            Some(Value::Number(n)) => return n.as_f64().unwrap_or(0.0),
            Some(Value::String(s)) => {
                if let Ok(f) = s.parse::<f64>() {
                    return f;
                }
            }
            _ => {}
        }
    }
    0.0
}

fn first_u64(v: &Value, keys: &[&str]) -> u64 {
    first_num(v, keys).max(0.0) as u64
}

/// First truthy signal among alias keys (`bool`, 0/1, "true"/"yes").
fn first_bool(v: &Value, keys: &[&str]) -> bool {
    for k in keys {
        match v.get(*k) {
            Some(Value::Bool(b)) => return *b,
            Some(Value::Number(n)) => return n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => return true,
                "false" | "0" | "no" => return false,
                _ => {}
            },
            _ => {}
        }
    }
    false
}

/// Percent-vs-fraction: magnitudes above 1.0 are read as 0–100 percentages.
fn ratio_of(v: &Value, keys: &[&str]) -> f64 {
    let n = first_num(v, keys);
    if n > 1.0 { n / 100.0 } else { n }
}

/// Epoch magnitude heuristic: ≥1e15 µs, ≥1e12 ms, otherwise seconds.
fn epoch_secs(f: f64) -> i64 {
    let div = if f >= 1e15 {
        1e6
    } else if f >= 1e12 {
        1e3
    } else {
        1.0
    };
    (f / div) as i64
}

/// Datetimes tolerated: RFC3339 strings, `YYYY-MM-DD HH:MM:SS`, epoch numbers.
fn first_time(v: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    for k in keys {
        match v.get(*k) {
            Some(Value::Number(_)) => {
                let f = v.get(*k).and_then(Value::as_f64)?;
                return DateTime::from_timestamp(epoch_secs(f), 0);
            }
            Some(Value::String(s)) => {
                if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                    return Some(dt.with_timezone(&Utc));
                }
                if let Ok(n) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                    return Some(n.and_utc());
                }
                if let Ok(f) = s.parse::<f64>() {
                    return DateTime::from_timestamp(epoch_secs(f), 0);
                }
            }
            _ => {}
        }
    }
    None
}

/// Top-level row list tolerated as a bare array, an object with an array under
/// a common wrapper key (`data`/`rows`/`result`/`items`), or any array field.
/// `None` ⇒ shape unusable.
fn rows_of(v: &Value) -> Option<Vec<&Value>> {
    const WRAPPERS: &[&str] = &["data", "rows", "result", "items"];
    if let Some(a) = v.as_array() {
        return Some(a.iter().collect());
    }
    let o = v.as_object()?;
    for k in WRAPPERS {
        if let Some(a) = o.get(*k).and_then(Value::as_array) {
            return Some(a.iter().collect());
        }
    }
    o.values()
        .find_map(Value::as_array)
        .map(|a| a.iter().collect())
}

fn query_row_of(v: &Value) -> Option<QueryRow> {
    let obj = v.as_object()?;
    // Bare `elapsed` is seconds on dashboard payloads; `_ms` variants pass through.
    let elapsed_ms = if obj.contains_key("elapsed_ms") || obj.contains_key("duration_ms") {
        first_num(v, &["elapsed_ms", "duration_ms"])
    } else {
        first_num(v, &["elapsed", "duration", "elapsed_sec"]) * 1000.0
    };
    Some(QueryRow {
        id: first_str(v, &["query_id", "id"]),
        user: first_str(v, &["user", "user_name", "username"]),
        elapsed_ms,
        memory_bytes: first_u64(v, &["memory_bytes", "memory_usage", "memory"]),
        read_rows: first_u64(v, &["read_rows", "rows_read", "rows"]),
        read_bytes: first_u64(v, &["read_bytes", "bytes_read", "bytes"]),
        exception: obj
            .get("exception")
            .or_else(|| obj.get("error"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        normalized_sql: first_str(
            v,
            &["normalized_sql", "normalized", "sql", "query_text", "query"],
        ),
        started_at: first_time(
            v,
            &[
                "started_at",
                "event_time",
                "event_time_microseconds",
                "start",
            ],
        ),
    })
}

fn merge_row_of(v: &Value) -> Option<MergeRow> {
    v.as_object()?;
    Some(MergeRow {
        database: first_str(v, &["database", "db"]),
        table: first_str(v, &["table", "table_name"]),
        is_mutation: first_bool(v, &["is_mutation", "mutation"]),
        progress: ratio_of(v, &["progress", "progress_pct", "progress_percent"]) as f32,
        num_parts: first_u64(v, &["num_parts", "parts"]),
        total_memory_bytes: first_u64(v, &["total_memory_bytes", "memory_usage", "memory"]),
        elapsed_sec: first_num(v, &["elapsed_sec", "elapsed_seconds", "elapsed"]),
    })
}

fn replica_row_of(v: &Value) -> Option<ReplicaRow> {
    v.as_object()?;
    Some(ReplicaRow {
        database: first_str(v, &["database", "db"]),
        table: first_str(v, &["table", "table_name"]),
        replica_name: first_str(v, &["replica_name", "name", "host_name"]),
        is_readonly: first_bool(v, &["is_readonly", "readonly"]),
        is_session_expired: first_bool(v, &["is_session_expired", "session_expired"]),
        absolute_delay_sec: first_num(
            v,
            &["absolute_delay_sec", "absolute_delay", "delay_sec", "delay"],
        ),
        queue_size: first_u64(v, &["queue_size", "queue"]),
        inserts_in_queue: first_u64(v, &["inserts_in_queue", "inserts"]),
        merges_in_queue: first_u64(v, &["merges_in_queue", "merges"]),
    })
}

fn table_row_of(v: &Value) -> Option<TableStat> {
    v.as_object()?;
    Some(TableStat {
        database: first_str(v, &["database", "db"]),
        name: first_str(v, &["name", "table"]),
        engine: first_str(v, &["engine"]),
        parts: first_u64(v, &["parts", "parts_count"]),
        rows: first_u64(v, &["rows", "total_rows"]),
        bytes_on_disk: first_u64(v, &["bytes_on_disk", "total_bytes", "bytes", "disk_bytes"]),
        compressed_ratio: first_num(v, &["compressed_ratio", "compress_ratio"]),
        last_modified: first_time(v, &["last_modified", "modification_time", "modified_at"]),
    })
}

fn overview_of(v: &Value) -> Overview {
    Overview {
        qps: first_num(v, &["qps", "queries_per_sec"]),
        running_queries: first_u64(v, &["running_queries", "running"]),
        slow_queries_24h: first_u64(v, &["slow_queries_24h", "slow_queries"]),
        failed_queries_24h: first_u64(v, &["failed_queries_24h", "failed_queries"]),
        active_merges: first_u64(v, &["active_merges", "merges"]),
        replicas_ok: first_u64(v, &["replicas_ok", "replicas_alive"]),
        replicas_total: first_u64(v, &["replicas_total", "total_replicas"]),
        tables_total: first_u64(v, &["tables_total", "tables"]),
        parts_total: first_u64(v, &["parts_total", "parts"]),
        disk_used_bytes: first_u64(v, &["disk_used_bytes", "disk_used", "used_bytes"]),
        disk_total_bytes: first_u64(v, &["disk_total_bytes", "disk_total", "total_bytes"]),
        uptime_seconds: first_u64(v, &["uptime_seconds", "uptime"]),
        clickhouse_version: first_str(v, &["clickhouse_version", "version", "ch_version"]),
    }
}

fn health_of(v: &Value) -> Health {
    let obj = v.as_object();
    // A 2xx answer counts as ok unless `ok`/`status` in the payload say otherwise.
    let ok = match obj.and_then(|o| o.get("ok")) {
        Some(b) => b.as_bool().unwrap_or(false),
        None => match obj.and_then(|o| o.get("status")) {
            Some(s) => s.as_str().is_some_and(|s| s.eq_ignore_ascii_case("ok")),
            None => true,
        },
    };
    Health {
        ok,
        readonly_tables: first_u64(v, &["readonly_tables", "readonly"]),
        replication_lag_max_sec: first_num(
            v,
            &[
                "replication_lag_max_sec",
                "max_replication_lag",
                "lag_sec",
                "lag",
            ],
        ),
        zookeeper_available: first_bool(v, &["zookeeper_available", "zk_available"]),
        delayed_inserts: first_u64(v, &["delayed_inserts"]),
        distributed_files_to_insert: first_u64(v, &["distributed_files_to_insert"]),
        background_pool_utilization: ratio_of(
            v,
            &[
                "background_pool_utilization",
                "background_pool_size_ratio",
                "pool_utilization",
            ],
        ) as f32,
    }
}

/// Chart bucket forms tolerated: `{t_ms|t|ts, v|value}` objects or
/// `[epoch, value]` pairs; sub-1e12 stamps are seconds.
fn points_of(v: &Value) -> Vec<SeriesPoint> {
    let Some(a) = v.as_array() else {
        return Vec::new();
    };
    a.iter()
        .filter_map(|p| {
            if let Some(pair) = p.as_array() {
                let t = pair.first()?.as_f64()?;
                let val = pair.get(1)?.as_f64()?;
                return Some((t, val));
            }
            Some((
                first_num(p, &["t_ms", "t", "time", "ts", "timestamp"]),
                first_num(p, &["v", "value", "val"]),
            ))
        })
        .map(|(t, val)| {
            let ms = if t.abs() >= 1e12 { t } else { t * 1000.0 };
            SeriesPoint {
                t_ms: ms as i64,
                v: val,
            }
        })
        .collect()
}

/// Named series lookup tolerating direct arrays and `series:`/`data:` wrappers.
fn series_of(v: &Value, keys: &[&str]) -> Vec<SeriesPoint> {
    for node in [Some(v), v.get("series"), v.get("data")] {
        let Some(node) = node else { continue };
        if node.as_array().is_some_and(|a| !a.is_empty()) {
            let pts = points_of(node);
            if !pts.is_empty() {
                return pts;
            }
            continue;
        }
        for k in keys {
            if let Some(arr) = node.get(*k)
                && arr.as_array().is_some_and(|a| !a.is_empty())
            {
                let pts = points_of(arr);
                if !pts.is_empty() {
                    return pts;
                }
            }
        }
    }
    Vec::new()
}

#[async_trait]
impl DataSource for CloudClient {
    fn label(&self) -> String {
        format!("cloud: {}", self.base_url)
    }

    fn engine(&self) -> chm_core::SourceEngine {
        chm_core::SourceEngine::Cloud
    }

    /// GET /api/healthz (the one known-good route); any 2xx = reachable+authed.
    async fn ping(&self) -> Result<()> {
        let resp = self.request("healthz").await?;
        match resp.status().is_success() {
            true => Ok(()),
            false => Err(status_err(resp.status(), "healthz")),
        }
    }

    /// GET /api/overview, best-effort: absent/unknown fields keep defaults.
    async fn overview(&self, _range: TimeRange) -> Result<Overview> {
        let v = self.get_raw("overview").await?;
        Ok(overview_of(&v))
    }

    /// GET /api/traffic; buckets as {t,v} objects or [epoch,value] pairs.
    async fn traffic(&self, _range: TimeRange) -> Result<TrafficSeries> {
        let v = self.get_raw("traffic").await?;
        Ok(TrafficSeries {
            queries_per_sec: series_of(&v, &["queries_per_sec", "qps", "queries"]),
            rows_read_per_sec: series_of(&v, &["rows_read_per_sec", "rows_read"]),
            network_rx_bps: series_of(&v, &["network_rx_bps", "rx_bps", "network_rx"]),
            network_tx_bps: series_of(&v, &["network_tx_bps", "tx_bps", "network_tx"]),
        })
    }

    /// GET /api/running-queries → QueryRow[] via alias-tolerant row mapping.
    async fn running_queries(&self) -> Result<Vec<QueryRow>> {
        self.row_list("running-queries", query_row_of).await
    }

    /// GET /api/slow-queries → QueryRow[] (same tolerant mapping).
    async fn slow_queries(&self, _range: TimeRange) -> Result<Vec<QueryRow>> {
        self.row_list("slow-queries", query_row_of).await
    }

    /// GET /api/failed-queries → QueryRow[] (`exception`/`error` kept as-is).
    async fn failed_queries(&self, _range: TimeRange) -> Result<Vec<QueryRow>> {
        self.row_list("failed-queries", query_row_of).await
    }

    /// GET /api/merges → MergeRow[]; progress auto-scaled from percent.
    async fn merges(&self) -> Result<Vec<MergeRow>> {
        self.row_list("merges", merge_row_of).await
    }

    /// GET /api/replicas → ReplicaRow[] with boolean/string coercion.
    async fn replicas(&self) -> Result<Vec<ReplicaRow>> {
        self.row_list("replicas", replica_row_of).await
    }

    /// GET /api/health; 2xx-without-payload counts as ok.
    async fn health(&self) -> Result<Health> {
        let v = self.get_raw("health").await?;
        Ok(health_of(&v))
    }

    /// GET /api/tables → TableStat[]; datetimes via RFC3339/SQL/epoch parse.
    async fn tables(&self) -> Result<Vec<TableStat>> {
        self.row_list("tables", table_row_of).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(uri: &str) -> CloudClient {
        CloudClient::new(uri, Some("sk-test".into()))
    }

    async fn server_with(mocks: Vec<Mock>) -> MockServer {
        let srv = MockServer::start().await;
        for m in mocks {
            m.mount(&srv).await;
        }
        srv
    }

    #[tokio::test]
    async fn ping_ok_on_healthz_200() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/healthz"))
                .respond_with(ResponseTemplate::new(200).set_body_string("ok")),
        ])
        .await;
        assert!(client(&srv.uri()).ping().await.is_ok());
    }

    #[tokio::test]
    async fn ping_401_is_auth_error() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/healthz"))
                .respond_with(ResponseTemplate::new(401)),
        ])
        .await;
        match client(&srv.uri()).ping().await {
            Err(DataSourceError::Auth { .. }) => {}
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ping_500_is_query_error() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/healthz"))
                .respond_with(ResponseTemplate::new(500)),
        ])
        .await;
        match client(&srv.uri()).ping().await {
            Err(DataSourceError::Query { .. }) => {}
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_raw_passthrough_sends_path_query_and_auth_header() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/custom/route"))
                .and(query_param("range", "24h"))
                .and(header("authorization", "Bearer sk-test"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"echo": true}))),
        ])
        .await;
        let got = client(&srv.uri())
            .get_raw("custom/route?range=24h")
            .await
            .unwrap();
        assert_eq!(got, json!({"echo": true}));
    }

    #[tokio::test]
    async fn overview_maps_full_payload() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/overview"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "qps": 12.5,
                    "running_queries": 3,
                    "uptime_seconds": 99,
                    "clickhouse_version": "24.3.1"
                }))),
        ])
        .await;
        let ov = client(&srv.uri())
            .overview(TimeRange::OneHour)
            .await
            .unwrap();
        assert_eq!(ov.qps, 12.5);
        assert_eq!(ov.running_queries, 3);
        assert_eq!(ov.uptime_seconds, 99);
        assert_eq!(ov.clickhouse_version, "24.3.1");
        assert_eq!(ov.tables_total, 0); // absent fields keep defaults
    }

    #[tokio::test]
    async fn overview_sparse_payload_yields_defaults() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/overview"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({}))),
        ])
        .await;
        let ov = client(&srv.uri())
            .overview(TimeRange::TwentyFourHours)
            .await
            .unwrap();
        assert_eq!(ov, Overview::default());
    }

    #[tokio::test]
    async fn running_queries_maps_both_row_styles() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/running-queries"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [
                        {"query_id": "q-1", "user": "default", "elapsed": 1.5, "memory_bytes": 1024},
                        {"id": "q-2", "user": "etl", "elapsed_ms": 250, "read_rows": 7}
                    ]
                }))),
        ])
        .await;
        let rows = client(&srv.uri()).running_queries().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "q-1");
        assert_eq!(rows[0].elapsed_ms, 1500.0); // seconds → ms
        assert_eq!(rows[1].id, "q-2"); // `id` alias
        assert_eq!(rows[1].elapsed_ms, 250.0); // `_ms` passthrough
        assert_eq!(rows[1].read_rows, 7);
    }

    #[tokio::test]
    async fn running_queries_rejects_unusable_shape() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/running-queries"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true}))),
        ])
        .await;
        match client(&srv.uri()).running_queries().await {
            Err(DataSourceError::Query { .. }) => {}
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn base_url_trailing_slash_is_normalized() {
        let srv = server_with(vec![
            Mock::given(method("GET"))
                .and(path("/api/healthz"))
                .respond_with(ResponseTemplate::new(200)),
        ])
        .await;
        let c = CloudClient::new(format!("{}/", srv.uri()), None);
        assert!(c.ping().await.is_ok());
    }

    #[tokio::test]
    async fn refused_socket_maps_to_connection_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // port now refused
        let c = CloudClient::new(format!("http://{addr}"), None);
        match c.ping().await {
            Err(DataSourceError::Connection { .. }) => {}
            other => panic!("expected Connection, got {other:?}"),
        }
    }
}
