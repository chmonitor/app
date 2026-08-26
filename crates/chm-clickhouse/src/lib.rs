//! Mode 2 — direct ClickHouse HTTP client. Speaks JSONEachRow over
//! `POST {url}/?default_format=JSONEachRow` with optional basic auth.
//! SQL ported from chmonitor's packages/sql-builder queries. The DataSource
//! impl reproduces async-trait's desugared signatures directly so this crate
//! needs no async-trait dependency of its own.

use std::future::Future;
use std::pin::Pin;

use chm_core::{
    DataSource, DataSourceError, Health, MergeRow, Overview, QueryRow, ReplicaRow, Result,
    TableStat, TimeRange, TrafficSeries,
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;

#[derive(Debug, Clone)]
pub struct ClickHouseClient {
    url: String,
    user: String,
    password: Option<String>,
    http: reqwest::Client,
}

impl ClickHouseClient {
    pub fn new(url: impl Into<String>, user: impl Into<String>, password: Option<String>) -> Self {
        Self {
            url: url.into().trim_end_matches('/').to_string(),
            user: user.into(),
            password,
            http: reqwest::Client::new(),
        }
    }

    /// POST a SELECT, parse JSONEachRow lines into T.
    pub async fn query_rows<T: DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        let mut req = self
            .http
            .post(format!("{}/?default_format=JSONEachRow", self.url))
            .basic_auth(&self.user, self.password.clone())
            .body(sql.to_string());
        req = req.header("X-ClickHouse-User", &self.user);
        let resp = req.send().await.map_err(|e| DataSourceError::Connection {
            message: e.to_string(),
        })?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| DataSourceError::Query {
            message: e.to_string(),
        })?;
        if !status.is_success() {
            return Err(DataSourceError::Query {
                message: format!("CH {}: {}", status.as_u16(), body.trim()),
            });
        }
        body.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str(l).map_err(|e| DataSourceError::Query {
                    message: format!("row decode: {e}: {l}"),
                })
            })
            .collect()
    }

    async fn scalar_u64(&self, sql: &str) -> Result<u64> {
        let rows: Vec<raw::Scalar> = self.query_rows(sql).await?;
        Ok(rows.first().map(|r| r.v).unwrap_or(0))
    }
}

// --- SQL -------------------------------------------------------------------
// Placeholders __FROM__/__TO__ receive 'YYYY-MM-DD hh:mm:ss' UTC literals;
// __BUCKET_SEC__ receives the traffic bucket width. System-table identifiers
// below are compile-time constants (never user input), so no runtime quoting
// is required.

/// chmonitor dashboard running-queries (system.processes).
pub const Q_RUNNING: &str = r#"
SELECT query_id, user, elapsed, memoryUsage AS memory_usage,
       read_rows, read_bytes, normalizeQuery(query) AS normalized_query
FROM system.processes
ORDER BY elapsed DESC
LIMIT 100"#;

/// chmonitor dashboard overview headline block.
/// Running / schema counts match `apps/dashboard/src/lib/api/charts/overview-charts.ts`.
pub const Q_OVERVIEW_MAIN: &str = r#"
SELECT
  (SELECT count() FROM system.processes WHERE is_cancelled = 0) AS running_queries,
  (SELECT count() FROM system.merges) + (SELECT countIf(NOT is_done) FROM system.mutations) AS active_merges,
  (SELECT countDistinct(database) FROM system.tables
     WHERE lower(database) NOT IN ('system', 'information_schema')) AS databases_total,
  (SELECT countDistinct(format('{}.{}', database, name)) FROM system.tables
     WHERE lower(database) NOT IN ('system', 'information_schema')) AS tables_total,
  (SELECT count() FROM system.parts WHERE active) AS parts_total,
  (SELECT coalesce(sum(bytes_on_disk), 0) FROM system.parts WHERE active) AS disk_used_bytes,
  uptime() AS uptime_seconds,
  version() AS clickhouse_version"#;

/// Dashboard `query-count-today`: QueryFinish rows since local midnight.
pub const Q_OVERVIEW_TODAY: &str = r#"
SELECT count() AS v
FROM system.query_log
WHERE type = 'QueryFinish' AND toDate(event_time) = today()"#;

/// qps denominator comes from the caller-selected range window.
pub const Q_OVERVIEW_QPS: &str = r#"
SELECT countIf(type = 'QueryStart') AS started_queries
FROM system.query_log
WHERE event_time >= toDateTime('__FROM__') AND event_time < toDateTime('__TO__')"#;

/// chmonitor dashboard slow/failed counters, fixed trailing 24h window.
pub const Q_OVERVIEW_LOG_COUNTS_24H: &str = r#"
SELECT
  countIf(type = 'QueryFinish' AND query_duration_ms > 5000) AS slow_queries_24h,
  countIf(exception != '') AS failed_queries_24h
FROM system.query_log
WHERE event_time >= toDateTime('__FROM__') AND event_time < toDateTime('__TO__')"#;

pub const Q_OVERVIEW_REPLICAS: &str = "SELECT count() AS replicas_total, \
     countIf(is_readonly = 0 AND is_session_expired = 0) AS replicas_ok \
     FROM system.replicas";

pub const Q_OVERVIEW_DISKS: &str =
    "SELECT coalesce(sum(total_space), 0) AS v FROM system.disks WHERE total_space > 0";

/// chmonitor dashboard traffic chart; toStartOfInterval bucketing.
pub const Q_TRAFFIC: &str = r#"
SELECT
  toInt64(toUnixTimestamp(toStartOfInterval(event_time, INTERVAL __BUCKET_SEC__ SECOND))) * 1000 AS t_ms,
  count() AS queries,
  sum(read_rows) AS rows_read,
  sum(read_bytes) AS rx_bytes,
  sum(written_bytes) AS tx_bytes
FROM system.query_log
WHERE event_time >= toDateTime('__FROM__') AND event_time < toDateTime('__TO__') AND type = 'QueryFinish'
GROUP BY t_ms
ORDER BY t_ms"#;

/// chmonitor dashboard slow-queries; duration > threshold, newest first.
pub const Q_SLOW_QUERIES: &str = r#"
SELECT
  query_id, user,
  query_duration_ms AS duration_ms,
  memory_usage,
  read_rows, read_bytes,
  '' AS exception,
  normalizeQuery(query) AS normalized_query,
  event_time AS started_at
FROM system.query_log
WHERE type = 'QueryFinish'
  AND query_duration_ms > 5000
  AND event_time >= toDateTime('__FROM__')
  AND event_time <= toDateTime('__TO__')
ORDER BY duration DESC
LIMIT 100"#;

/// chmonitor dashboard failed-queries.
pub const Q_FAILED_QUERIES: &str = r#"
SELECT
  query_id, user,
  query_duration_ms AS duration_ms,
  memory_usage,
  read_rows, read_bytes,
  exception,
  normalizeQuery(query) AS normalized_query,
  event_time AS started_at
FROM system.query_log
WHERE exception != ''
  AND event_time >= toDateTime('__FROM__')
  AND event_time <= toDateTime('__TO__')
ORDER BY event_time DESC
LIMIT 100"#;

/// chmonitor merges + mutations pages unified.
pub const Q_MERGES: &str = r#"
SELECT database, table, is_mutation, progress, num_parts, memory_bytes, elapsed_sec
FROM
(
  SELECT database, table, 0 AS is_mutation, round(progress, 4) AS progress,
         num_parts, total_memory_amount AS memory_bytes, round(elapsed, 3) AS elapsed_sec
  FROM system.merges
  UNION ALL
  SELECT database, table, 1 AS is_mutation, 0 AS progress,
         parts_to_do AS num_parts, 0 AS memory_bytes, 0 AS elapsed_sec
  FROM system.mutations
  WHERE NOT is_done
)
ORDER BY elapsed_sec DESC"#;

/// chmonitor dashboard replicas page.
pub const Q_REPLICAS: &str = r#"
SELECT database, table, replica_name,
       is_readonly, is_session_expired,
       absolute_delay, queue_size, inserts_in_queue, merges_in_queue
FROM system.replicas
ORDER BY absolute_delay DESC"#;

/// chmonitor health card composite (readonly tables, lag, inserts, pool).
pub const Q_HEALTH: &str = r#"
SELECT
  (SELECT count() FROM system.tables WHERE engine LIKE '%Replicated%' AND is_readonly = 1) AS readonly_tables,
  (SELECT max(absolute_delay) FROM system.replicas) AS replication_lag_max_sec,
  (SELECT value FROM system.metrics WHERE metric = 'DelayedInserts') AS delayed_inserts,
  (SELECT value FROM system.metrics WHERE metric = 'DistributedFilesToInsert') AS distributed_files_to_insert,
  (SELECT value FROM system.metrics WHERE metric = 'BackgroundPoolTask') AS background_pool_tasks"#;

/// Keeper/zookeeper availability probe; returns result=1/0.
pub const Q_ZOOKEEPER: &str = "EXISTS TABLE system.zookeeper";

/// background_pool_size lives in system.server_settings on 24.2+.
pub const Q_POOL_SIZE: &str =
    "SELECT value AS pool_size FROM system.server_settings WHERE name = 'background_pool_size'";

/// chmonitor tables page: parts aggregate joined with engines.
pub const Q_TABLES: &str = r#"
SELECT
  p.database AS database,
  p.table AS name,
  any(t.engine) AS engine,
  count() AS parts,
  sum(p.rows) AS total_rows,
  sum(p.bytes_on_disk) AS bytes_on_disk,
  round(sum(p.data_uncompressed_bytes) / greatest(toFloat64(sum(p.data_compressed_bytes)), 1), 3) AS compressed_ratio,
  max(p.modification_time) AS last_modified
FROM system.parts AS p
ANY LEFT JOIN system.tables AS t ON t.database = p.database AND t.name = p.table
WHERE p.active
GROUP BY p.database, p.table
ORDER BY bytes_on_disk DESC
LIMIT 200"#;

/// Millisecond bucket widths per dashboard range selection.
pub(crate) fn bucket_seconds(range: TimeRange) -> i64 {
    match range {
        TimeRange::OneHour | TimeRange::SixHours => 60,
        TimeRange::TwentyFourHours => 300,
        TimeRange::SevenDays => 1800,
        TimeRange::ThirtyDays => 3600,
    }
}

fn fmt_ts(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn render_sql(tpl: &str, params: &[(&str, String)]) -> String {
    let mut sql = tpl.to_string();
    for (marker, value) in params {
        sql = sql.replace(marker, value);
    }
    sql
}

fn with_window(tpl: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> String {
    render_sql(tpl, &[("__FROM__", fmt_ts(from)), ("__TO__", fmt_ts(to))])
}

impl DataSource for ClickHouseClient {
    fn label(&self) -> String {
        format!("clickhouse: {}", self.url)
    }

    fn ping<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let mut req = self
                .http
                .post(format!("{}/?query=SELECT%201", self.url))
                .basic_auth(&self.user, self.password.clone());
            req = req.header("X-ClickHouse-User", &self.user);
            let resp = req.send().await.map_err(|e| DataSourceError::Connection {
                message: e.to_string(),
            })?;
            match resp.status().is_success() {
                true => Ok(()),
                false => Err(DataSourceError::Auth {
                    message: format!("ping {}", resp.status().as_u16()),
                }),
            }
        })
    }

    fn overview<'life0, 'async_trait>(
        &'life0 self,
        range: TimeRange,
    ) -> Pin<Box<dyn Future<Output = Result<Overview>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let now = Utc::now();
            let (win_from, win_to) = range.window(now);
            let day_from = win_to - chrono::Duration::hours(24);

            let main: Vec<raw::OverviewMain> = self.query_rows(Q_OVERVIEW_MAIN).await?;
            let qps_row: Vec<raw::StartedQueries> = self
                .query_rows(&with_window(Q_OVERVIEW_QPS, win_from, win_to))
                .await?;
            let counts: Vec<raw::LogCounts> = self
                .query_rows(&with_window(Q_OVERVIEW_LOG_COUNTS_24H, day_from, win_to))
                .await?;
            let reps: Vec<raw::ReplicaCounts> = self.query_rows(Q_OVERVIEW_REPLICAS).await?;
            let disk_total_bytes = self.scalar_u64(Q_OVERVIEW_DISKS).await?;
            let queries_today = self.scalar_u64(Q_OVERVIEW_TODAY).await.unwrap_or(0);

            let main = main.into_iter().next().unwrap_or_default();
            let qps_row = qps_row.into_iter().next().unwrap_or_default();
            let counts = counts.into_iter().next().unwrap_or_default();
            let reps = reps.into_iter().next().unwrap_or_default();

            let range_secs = range.duration().num_seconds().max(1) as f64;
            Ok(Overview {
                qps: qps_row.started_queries as f64 / range_secs,
                running_queries: main.running_queries,
                slow_queries_24h: counts.slow_queries_24h,
                failed_queries_24h: counts.failed_queries_24h,
                active_merges: main.active_merges,
                replicas_ok: reps.replicas_ok,
                replicas_total: reps.replicas_total,
                tables_total: main.tables_total,
                parts_total: main.parts_total,
                disk_used_bytes: main.disk_used_bytes,
                disk_total_bytes,
                uptime_seconds: main.uptime_seconds,
                clickhouse_version: main.clickhouse_version,
                databases_total: main.databases_total,
                queries_today,
            })
        })
    }

    fn traffic<'life0, 'async_trait>(
        &'life0 self,
        range: TimeRange,
    ) -> Pin<Box<dyn Future<Output = Result<TrafficSeries>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (from, to) = range.window(Utc::now());
            let tpl = render_sql(
                Q_TRAFFIC,
                &[("__BUCKET_SEC__", bucket_seconds(range).to_string())],
            );
            let sql = with_window(&tpl, from, to);
            let buckets: Vec<raw::TrafficBucket> = self.query_rows(&sql).await?;

            let secs = bucket_seconds(range) as f64;
            let mut s = TrafficSeries::default();
            for b in buckets {
                let point = |v: f64| chm_core::SeriesPoint { t_ms: b.t_ms, v };
                s.queries_per_sec.push(point(b.queries as f64 / secs));
                s.rows_read_per_sec.push(point(b.rows_read as f64 / secs));
                s.network_rx_bps.push(point(b.rx_bytes as f64 * 8.0 / secs));
                s.network_tx_bps.push(point(b.tx_bytes as f64 * 8.0 / secs));
            }
            Ok(s)
        })
    }

    fn running_queries<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<QueryRow>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let rows: Vec<raw::RunningQuery> = self.query_rows(Q_RUNNING).await?;
            Ok(rows
                .into_iter()
                .map(|r| QueryRow {
                    id: r.query_id,
                    user: r.user,
                    elapsed_ms: r.elapsed * 1000.0,
                    memory_bytes: r.memory_usage,
                    read_rows: r.read_rows,
                    read_bytes: r.read_bytes,
                    exception: None,
                    normalized_sql: r.normalized_query,
                    started_at: None,
                })
                .collect())
        })
    }

    fn slow_queries<'life0, 'async_trait>(
        &'life0 self,
        range: TimeRange,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<QueryRow>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (from, to) = range.window(Utc::now());
            let sql = with_window(Q_SLOW_QUERIES, from, to);
            let rows: Vec<raw::QueryLogRow> = self.query_rows(&sql).await?;
            Ok(rows
                .into_iter()
                .map(raw::QueryLogRow::into_domain)
                .collect())
        })
    }

    fn failed_queries<'life0, 'async_trait>(
        &'life0 self,
        range: TimeRange,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<QueryRow>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let (from, to) = range.window(Utc::now());
            let sql = with_window(Q_FAILED_QUERIES, from, to);
            let rows: Vec<raw::QueryLogRow> = self.query_rows(&sql).await?;
            Ok(rows
                .into_iter()
                .map(raw::QueryLogRow::into_domain)
                .collect())
        })
    }

    fn merges<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MergeRow>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let rows: Vec<raw::MergeUnionRow> = self.query_rows(Q_MERGES).await?;
            Ok(rows
                .into_iter()
                .map(|r| MergeRow {
                    database: r.database,
                    table: r.table,
                    is_mutation: r.is_mutation != 0,
                    progress: r.progress as f32,
                    num_parts: r.num_parts,
                    total_memory_bytes: r.memory_bytes,
                    elapsed_sec: r.elapsed_sec,
                })
                .collect())
        })
    }

    fn replicas<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ReplicaRow>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let rows: Vec<raw::ReplicaSystemRow> = self.query_rows(Q_REPLICAS).await?;
            Ok(rows
                .into_iter()
                .map(|r| ReplicaRow {
                    database: r.database,
                    table: r.table,
                    replica_name: r.replica_name,
                    is_readonly: r.is_readonly != 0,
                    is_session_expired: r.is_session_expired != 0,
                    absolute_delay_sec: r.absolute_delay,
                    queue_size: r.queue_size,
                    inserts_in_queue: r.inserts_in_queue,
                    merges_in_queue: r.merges_in_queue,
                })
                .collect())
        })
    }

    fn health<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<Health>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let composite: Vec<raw::HealthComposite> = self.query_rows(Q_HEALTH).await?;
            let c = composite.into_iter().next().unwrap_or_default();

            let zk: Vec<raw::ZkProbe> = self.query_rows(Q_ZOOKEEPER).await?;
            let zookeeper_available = zk.first().map(|z| z.result != 0).unwrap_or(false);

            let pool_size: Option<u64> = match self.query_rows::<raw::PoolSize>(Q_POOL_SIZE).await {
                Ok(rows) => rows
                    .into_iter()
                    .next()
                    .map(|p| p.pool_size)
                    .filter(|s| *s > 0),
                Err(_) => None,
            };
            let background_pool_utilization = match pool_size {
                Some(size) => (c.background_pool_tasks as f64 / size as f64) as f32,
                None => 0.0,
            };

            let ok = c.readonly_tables == 0
                && c.replication_lag_max_sec <= 300.0
                && zookeeper_available
                && c.delayed_inserts == 0
                && c.distributed_files_to_insert == 0;

            Ok(Health {
                ok,
                readonly_tables: c.readonly_tables,
                replication_lag_max_sec: c.replication_lag_max_sec,
                zookeeper_available,
                delayed_inserts: c.delayed_inserts,
                distributed_files_to_insert: c.distributed_files_to_insert,
                background_pool_utilization,
            })
        })
    }

    fn tables<'life0, 'async_trait>(
        &'life0 self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TableStat>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let rows: Vec<raw::TableRow> = self.query_rows(Q_TABLES).await?;
            Ok(rows
                .into_iter()
                .map(|r| TableStat {
                    database: r.database,
                    name: r.name,
                    engine: r.engine.unwrap_or_default(),
                    parts: r.parts,
                    rows: r.total_rows,
                    bytes_on_disk: r.bytes_on_disk,
                    compressed_ratio: r.compressed_ratio,
                    last_modified: raw::parse_ch_dt(&r.last_modified),
                })
                .collect())
        })
    }
}

/// Raw row shapes returned by system tables before conversion to domain types.
/// Every field is `#[serde(default)]` so partial JSONEachRow rows degrade
/// instead of failing the whole poll cycle.
mod raw {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use serde::Deserialize;

    pub(super) fn parse_ch_dt(s: &str) -> Option<DateTime<Utc>> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        for fmt in [
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S%.f",
        ] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
                return Some(naive.and_utc());
            }
        }
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }

    #[derive(Debug, Deserialize)]
    pub struct Scalar {
        #[serde(default)]
        pub v: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct RunningQuery {
        #[serde(default)]
        pub query_id: String,
        #[serde(default)]
        pub user: String,
        #[serde(default)]
        pub elapsed: f64,
        #[serde(default)]
        pub memory_usage: u64,
        #[serde(default)]
        pub read_rows: u64,
        #[serde(default)]
        pub read_bytes: u64,
        #[serde(default)]
        pub normalized_query: String,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct OverviewMain {
        #[serde(default)]
        pub running_queries: u64,
        #[serde(default)]
        pub active_merges: u64,
        #[serde(default)]
        pub databases_total: u64,
        #[serde(default)]
        pub tables_total: u64,
        #[serde(default)]
        pub parts_total: u64,
        #[serde(default)]
        pub disk_used_bytes: u64,
        #[serde(default)]
        pub uptime_seconds: u64,
        #[serde(default)]
        pub clickhouse_version: String,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct StartedQueries {
        #[serde(default)]
        pub started_queries: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct LogCounts {
        #[serde(default)]
        pub slow_queries_24h: u64,
        #[serde(default)]
        pub failed_queries_24h: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct ReplicaCounts {
        #[serde(default)]
        pub replicas_ok: u64,
        #[serde(default)]
        pub replicas_total: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct TrafficBucket {
        #[serde(default)]
        pub t_ms: i64,
        #[serde(default)]
        pub queries: u64,
        #[serde(default)]
        pub rows_read: u64,
        #[serde(default)]
        pub rx_bytes: u64,
        #[serde(default)]
        pub tx_bytes: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct QueryLogRow {
        #[serde(default)]
        pub query_id: String,
        #[serde(default)]
        pub user: String,
        #[serde(default)]
        pub duration_ms: f64,
        #[serde(default)]
        pub memory_usage: u64,
        #[serde(default)]
        pub read_rows: u64,
        #[serde(default)]
        pub read_bytes: u64,
        #[serde(default)]
        pub exception: String,
        #[serde(default)]
        pub normalized_query: String,
        #[serde(default)]
        pub started_at: String,
    }

    impl QueryLogRow {
        pub(super) fn into_domain(self) -> chm_core::QueryRow {
            let exception = (!self.exception.is_empty()).then_some(self.exception);
            chm_core::QueryRow {
                id: self.query_id,
                user: self.user,
                elapsed_ms: self.duration_ms,
                memory_bytes: self.memory_usage,
                read_rows: self.read_rows,
                read_bytes: self.read_bytes,
                exception,
                normalized_sql: self.normalized_query,
                started_at: parse_ch_dt(&self.started_at),
            }
        }
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct MergeUnionRow {
        #[serde(default)]
        pub database: String,
        #[serde(default)]
        pub table: String,
        #[serde(default)]
        pub is_mutation: u8,
        #[serde(default)]
        pub progress: f64,
        #[serde(default)]
        pub num_parts: u64,
        #[serde(default)]
        pub memory_bytes: u64,
        #[serde(default)]
        pub elapsed_sec: f64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct ReplicaSystemRow {
        #[serde(default)]
        pub database: String,
        #[serde(default)]
        pub table: String,
        #[serde(default)]
        pub replica_name: String,
        #[serde(default)]
        pub is_readonly: u8,
        #[serde(default)]
        pub is_session_expired: u8,
        #[serde(default)]
        pub absolute_delay: f64,
        #[serde(default)]
        pub queue_size: u64,
        #[serde(default)]
        pub inserts_in_queue: u64,
        #[serde(default)]
        pub merges_in_queue: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct HealthComposite {
        #[serde(default)]
        pub readonly_tables: u64,
        #[serde(default)]
        pub replication_lag_max_sec: f64,
        #[serde(default)]
        pub delayed_inserts: u64,
        #[serde(default)]
        pub distributed_files_to_insert: u64,
        #[serde(default)]
        pub background_pool_tasks: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct ZkProbe {
        #[serde(default)]
        pub result: u8,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct PoolSize {
        #[serde(default)]
        pub pool_size: u64,
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct TableRow {
        #[serde(default)]
        pub database: String,
        #[serde(default)]
        pub name: String,
        // ANY LEFT JOIN can yield NULL for non-MergeTree engines.
        #[serde(default)]
        pub engine: Option<String>,
        #[serde(default)]
        pub parts: u64,
        #[serde(default)]
        pub total_rows: u64,
        #[serde(default)]
        pub bytes_on_disk: u64,
        #[serde(default)]
        pub compressed_ratio: f64,
        #[serde(default)]
        pub last_modified: String,
    }
}

#[cfg(test)]
mod sql_snapshots {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn running_query_hits_system_processes() {
        assert!(Q_RUNNING.contains("FROM system.processes"));
        assert!(Q_RUNNING.contains("normalizeQuery(query) AS normalized_query"));
        assert!(Q_RUNNING.contains("ORDER BY elapsed DESC"));
        assert!(Q_RUNNING.contains("LIMIT 100"));
    }

    #[test]
    fn overview_main_matches_dashboard_kpis() {
        assert!(Q_OVERVIEW_MAIN.contains("FROM system.processes WHERE is_cancelled = 0"));
        assert!(Q_OVERVIEW_MAIN.contains("NOT IN ('system', 'information_schema')"));
        assert!(Q_OVERVIEW_MAIN.contains("AS databases_total"));
        assert!(Q_OVERVIEW_TODAY.contains("toDate(event_time) = today()"));
        assert!(Q_OVERVIEW_TODAY.contains("type = 'QueryFinish'"));
    }

    #[test]
    fn slow_queries_shape() {
        assert!(Q_SLOW_QUERIES.contains("FROM system.query_log"));
        assert!(Q_SLOW_QUERIES.contains("type = 'QueryFinish'"));
        assert!(Q_SLOW_QUERIES.contains("query_duration_ms > 5000"));
        assert!(Q_SLOW_QUERIES.contains("ORDER BY duration DESC"));
        assert!(Q_SLOW_QUERIES.contains("LIMIT 100"));
    }

    #[test]
    fn failed_queries_filter_on_exception() {
        assert!(Q_FAILED_QUERIES.contains("FROM system.query_log"));
        assert!(Q_FAILED_QUERIES.contains("exception != ''"));
        assert!(Q_FAILED_QUERIES.contains("ORDER BY event_time DESC"));
    }

    #[test]
    fn merges_unions_mutations() {
        assert!(Q_MERGES.contains("FROM system.merges"));
        assert!(Q_MERGES.contains("UNION ALL"));
        assert!(Q_MERGES.contains("FROM system.mutations"));
        assert!(Q_MERGES.contains("WHERE NOT is_done"));
        assert!(Q_MERGES.contains("total_memory_amount AS memory_bytes"));
    }

    #[test]
    fn replicas_full_mapping() {
        for col in [
            "database",
            "table",
            "replica_name",
            "is_readonly",
            "is_session_expired",
            "absolute_delay",
            "queue_size",
            "inserts_in_queue",
            "merges_in_queue",
        ] {
            assert!(Q_REPLICAS.contains(col), "missing {col}");
        }
        assert!(Q_REPLICAS.contains("FROM system.replicas"));
    }

    #[test]
    fn health_composite_clauses() {
        assert!(Q_HEALTH.contains("engine LIKE '%Replicated%' AND is_readonly = 1"));
        assert!(Q_HEALTH.contains("max(absolute_delay)"));
        assert!(Q_HEALTH.contains("system.metrics WHERE metric = 'DelayedInserts'"));
        assert!(Q_HEALTH.contains("system.metrics WHERE metric = 'DistributedFilesToInsert'"));
        assert!(Q_HEALTH.contains("system.metrics WHERE metric = 'BackgroundPoolTask'"));
    }

    #[test]
    fn zookeeper_probe_is_exists_query() {
        assert_eq!(Q_ZOOKEEPER, "EXISTS TABLE system.zookeeper");
    }

    #[test]
    fn tables_aggregate_clauses() {
        assert!(Q_TABLES.contains("FROM system.parts AS p"));
        assert!(Q_TABLES.contains("ANY LEFT JOIN system.tables AS t"));
        assert!(Q_TABLES.contains("ON t.database = p.database AND t.name = p.table"));
        assert!(Q_TABLES.contains("WHERE p.active"));
        assert!(Q_TABLES.contains("GROUP BY p.database, p.table"));
        assert!(Q_TABLES.contains("sum(p.data_uncompressed_bytes)"));
        assert!(Q_TABLES.contains("AS compressed_ratio"));
        assert!(Q_TABLES.contains("ORDER BY bytes_on_disk DESC"));
    }

    #[test]
    fn traffic_buckets_by_interval() {
        assert!(
            Q_TRAFFIC.contains("toStartOfInterval(event_time, INTERVAL __BUCKET_SEC__ SECOND)")
        );
        assert!(Q_TRAFFIC.contains("GROUP BY t_ms"));
        assert!(Q_TRAFFIC.contains("__FROM__"));
        assert!(Q_TRAFFIC.contains("__TO__"));
    }

    #[test]
    fn window_rendering_substitutes_datetime_literals() {
        let from = Utc.with_ymd_and_hms(2026, 8, 22, 10, 0, 0).unwrap();
        let to = Utc.with_ymd_and_hms(2026, 8, 22, 11, 0, 0).unwrap();
        let sql = with_window(Q_OVERVIEW_QPS, from, to);
        assert!(!sql.contains("__FROM__"));
        assert!(!sql.contains("__TO__"));
        assert!(sql.contains("event_time >= toDateTime('2026-08-22 10:00:00')"));
        assert!(sql.contains("event_time < toDateTime('2026-08-22 11:00:00')"));
    }

    #[test]
    fn bucket_width_grows_with_range() {
        assert_eq!(bucket_seconds(TimeRange::OneHour), 60);
        assert_eq!(bucket_seconds(TimeRange::SixHours), 60);
        assert_eq!(bucket_seconds(TimeRange::TwentyFourHours), 300);
        assert_eq!(bucket_seconds(TimeRange::SevenDays), 1800);
        assert_eq!(bucket_seconds(TimeRange::ThirtyDays), 3600);
    }

    #[test]
    fn ch_datetime_parser_accepts_server_formats() {
        assert!(raw::parse_ch_dt("2026-08-22 10:00:00").is_some());
        assert!(raw::parse_ch_dt("2026-08-22T10:00:00").is_some());
        assert!(raw::parse_ch_dt("2026-08-22 10:00:00.123456").is_some());
        assert!(raw::parse_ch_dt("2026-08-22T10:00:00Z").is_some());
        assert!(raw::parse_ch_dt("").is_none());
        assert!(raw::parse_ch_dt("garbage").is_none());
    }
}

#[cfg(test)]
mod wiremock_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    type Routes = Arc<Vec<(&'static str, String)>>;

    /// Mount one catch-all POST / that routes on substrings of the SQL body,
    /// mirroring how CH would answer each statement; records seen bodies.
    async fn mount(
        server: &MockServer,
        routes: Vec<(&'static str, impl Into<String>)>,
    ) -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let routed: Routes = Arc::new(
            routes
                .into_iter()
                .map(|(needle, body)| (needle, body.into()))
                .collect(),
        );
        let captured = Arc::clone(&seen);
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(move |req: &Request| {
                let sql = String::from_utf8_lossy(&req.body).into_owned();
                captured.lock().expect("poisoned").push(sql.clone());
                let hit = routed
                    .iter()
                    .find(|(needle, _)| sql.contains(needle))
                    .map(|(_, body)| body.clone())
                    .unwrap_or_default();
                ResponseTemplate::new(200)
                    .set_body_string(hit)
                    .append_header("Content-Type", "application/x-ndjson")
            })
            .mount(server)
            .await;
        seen
    }

    async fn server_with(
        routes: Vec<(&'static str, impl Into<String>)>,
    ) -> (MockServer, ClickHouseClient, Arc<Mutex<Vec<String>>>) {
        let server = MockServer::start().await;
        let seen = mount(&server, routes).await;
        let client = ClickHouseClient::new(server.uri(), "default", None);
        (server, client, seen)
    }

    const RUNNING_BODY: &str = concat!(
        r#"{"query_id":"q-1","user":"analytics","elapsed":12.45,"memory_usage":4294967296,"read_rows":123,"read_bytes":456,"normalized_query":"SELECT count() FROM events"}"#,
        "\n",
        r#"{"query_id":"q-2","user":"etl","elapsed":3.12,"memory_usage":800,"read_rows":1,"read_bytes":2,"normalized_query":"INSERT INTO reports"}"#,
        "\n"
    );

    #[tokio::test]
    async fn running_queries_maps_domain_fields() {
        let (_s, c, seen) = server_with(vec![("FROM system.processes", RUNNING_BODY)]).await;
        let rows = c.running_queries().await.expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "q-1");
        assert_eq!(rows[0].user, "analytics");
        assert_eq!(rows[0].elapsed_ms, 12_450.0);
        assert_eq!(rows[0].memory_bytes, 4 << 30);
        assert_eq!(rows[0].read_rows, 123);
        assert_eq!(rows[0].exception, None);
        assert_eq!(rows[0].normalized_sql, "SELECT count() FROM events");
        let sql = last_sql(&seen);
        assert!(sql.contains("system.processes"), "sent: {sql}");
    }

    const SLOW_BODY: &str = concat!(
        r#"{"query_id":"s-1","user":"analyst","duration_ms":45000,"memory_usage":1000,"read_rows":9,"read_bytes":8,"exception":"","normalized_query":"SELECT 1","started_at":"2026-08-22 10:00:00"}"#,
        "\n"
    );

    #[tokio::test]
    async fn slow_queries_converts_duration_and_time() {
        let (_s, c, seen) = server_with(vec![("QueryFinish", SLOW_BODY)]).await;
        let rows = c
            .slow_queries(TimeRange::TwentyFourHours)
            .await
            .expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].elapsed_ms, 45_000.0);
        assert_eq!(rows[0].exception, None);
        let started = rows[0].started_at.expect("parsed ts");
        assert_eq!(
            started.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-08-22 10:00:00"
        );
        let sql = last_sql(&seen);
        for clause in [
            "type = 'QueryFinish'",
            "query_duration_ms > 5000",
            "ORDER BY duration DESC",
        ] {
            assert!(sql.contains(clause), "missing `{clause}` in {sql}");
        }
    }

    const FAILED_BODY: &str = concat!(
        r#"{"query_id":"f-1","user":"svc-ingest","duration_ms":210,"memory_usage":0,"read_rows":0,"read_bytes":0,"exception":"Table is readonly (replica delay)","normalized_query":"INSERT INTO ingest.events","started_at":"2026-08-22T10:05:00"}"#,
        "\n"
    );

    #[tokio::test]
    async fn failed_queries_surface_exception_text() {
        let (_s, c, _) = server_with(vec![("exception != ''", FAILED_BODY)]).await;
        let rows = c.failed_queries(TimeRange::SixHours).await.expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "f-1");
        assert_eq!(
            rows[0].exception.as_deref(),
            Some("Table is readonly (replica delay)")
        );
    }

    const TRAFFIC_BODY: &str = concat!(
        r#"{"t_ms":1779000000000,"queries":6000,"rows_read":9000000,"rx_bytes":30000000,"tx_bytes":15000000}"#,
        "\n",
        r#"{"t_ms":1779000060000,"queries":120,"rows_read":240000,"rx_bytes":600000,"tx_bytes":250000}"#,
        "\n"
    );

    #[tokio::test]
    async fn traffic_normalizes_buckets_to_rates() {
        let (_s, c, seen) = server_with(vec![("toStartOfInterval", TRAFFIC_BODY)]).await;
        let s = c.traffic(TimeRange::OneHour).await.expect("series");
        assert_eq!(s.queries_per_sec.len(), 2);
        assert_eq!(s.queries_per_sec[0].t_ms, 1_779_000_000_000);
        assert_eq!(s.queries_per_sec[0].v, 100.0);
        assert_eq!(s.rows_read_per_sec[0].v, 150_000.0);
        // bits/sec = bytes*8/60
        assert_eq!(s.network_rx_bps[0].v, 30_000_000.0 * 8.0 / 60.0);
        assert_eq!(s.network_tx_bps[1].v, 250_000.0 * 8.0 / 60.0);
        let sql = last_sql(&seen);
        assert!(sql.contains("INTERVAL 60 SECOND"), "sent: {sql}");
    }

    #[tokio::test]
    async fn traffic_bucket_width_scales_with_range() {
        let (_s, c, seen) = server_with(vec![("toStartOfInterval", TRAFFIC_BODY)]).await;
        c.traffic(TimeRange::ThirtyDays).await.expect("series");
        assert!(last_sql(&seen).contains("INTERVAL 3600 SECOND"));
    }

    const MERGES_BODY: &str = concat!(
        r#"{"database":"events","table":"clicks","is_mutation":0,"progress":0.62,"num_parts":148,"memory_bytes":943718400,"elapsed_sec":42.5}"#,
        "\n",
        r#"{"database":"events","table":"sessions","is_mutation":1,"progress":0,"num_parts":1,"memory_bytes":0,"elapsed_sec":0}"#,
        "\n"
    );

    #[tokio::test]
    async fn merges_union_maps_mutation_flag() {
        let (_s, c, _) = server_with(vec![("UNION ALL", MERGES_BODY)]).await;
        let rows = c.merges().await.expect("rows");
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].is_mutation);
        assert_eq!(rows[0].progress, 0.62);
        assert_eq!(rows[0].num_parts, 148);
        assert_eq!(rows[0].total_memory_bytes, 900 << 20);
        assert_eq!(rows[0].elapsed_sec, 42.5);
        assert!(rows[1].is_mutation);
        assert_eq!(rows[1].table, "sessions");
    }

    const REPLICAS_BODY: &str = concat!(
        r#"{"database":"events","table":"clicks","replica_name":"ch-1","is_readonly":0,"is_session_expired":0,"absolute_delay":0,"queue_size":0,"inserts_in_queue":0,"merges_in_queue":0}"#,
        "\n",
        r#"{"database":"logs","table":"app","replica_name":"ch-2","is_readonly":1,"is_session_expired":1,"absolute_delay":120.5,"queue_size":7,"inserts_in_queue":2,"merges_in_queue":3}"#,
        "\n"
    );

    #[tokio::test]
    async fn replicas_map_flags_and_queues() {
        let (_s, c, _) = server_with(vec![("FROM system.replicas", REPLICAS_BODY)]).await;
        let rows = c.replicas().await.expect("rows");
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].is_readonly);
        assert!(rows[1].is_readonly);
        assert!(rows[1].is_session_expired);
        assert_eq!(rows[1].absolute_delay_sec, 120.5);
        assert_eq!(rows[1].queue_size, 7);
        assert_eq!(rows[1].inserts_in_queue, 2);
        assert_eq!(rows[1].merges_in_queue, 3);
    }

    fn health_routes(tasks: u64, pool: u64, zk: u8) -> Vec<(&'static str, String)> {
        vec![
            (
                "BackgroundPoolTask",
                format!(
                    r#"{{"readonly_tables":0,"replication_lag_max_sec":1.5,"delayed_inserts":0,"distributed_files_to_insert":0,"background_pool_tasks":{tasks}}}"#
                ),
            ),
            ("EXISTS TABLE", format!(r#"{{"result":{zk}}}"#)),
            ("background_pool_size", format!(r#"{{"pool_size":{pool}}}"#)),
        ]
    }

    #[tokio::test]
    async fn health_ok_when_pool_size_known() {
        let (_s, c, _) = server_with(health_routes(34, 100, 1)).await;
        let h = c.health().await.expect("health");
        assert!(h.ok);
        assert_eq!(h.readonly_tables, 0);
        assert_eq!(h.replication_lag_max_sec, 1.5);
        assert!(h.zookeeper_available);
        assert_eq!(h.delayed_inserts, 0);
        assert_eq!(h.background_pool_utilization, 0.34);
    }

    #[tokio::test]
    async fn health_degrades_without_zookeeper_or_pool_setting() {
        let (_s, c, _) = server_with(health_routes(0, 0, 0)).await;
        let h = c.health().await.expect("health");
        assert!(!h.ok);
        assert!(!h.zookeeper_available);
        assert_eq!(h.background_pool_utilization, 0.0);
    }

    #[tokio::test]
    async fn health_flags_unhealthy_composite() {
        let routes: Vec<(&'static str, String)> = vec![
            (
                "BackgroundPoolTask",
                r#"{"readonly_tables":2,"replication_lag_max_sec":95.5,"delayed_inserts":3,"distributed_files_to_insert":12,"background_pool_tasks":90}"#.into(),
            ),
            ("EXISTS TABLE", r#"{"result":1}"#.into()),
            ("background_pool_size", r#"{"pool_size":100}"#.into()),
        ];
        let (_s, c, _) = server_with(routes).await;
        let h = c.health().await.expect("health");
        assert!(!h.ok);
        assert_eq!(h.readonly_tables, 2);
        assert_eq!(h.replication_lag_max_sec, 95.5);
        assert_eq!(h.delayed_inserts, 3);
        assert_eq!(h.distributed_files_to_insert, 12);
        assert_eq!(h.background_pool_utilization, 0.9);
    }

    const TABLES_BODY: &str = concat!(
        r#"{"database":"events","name":"clicks","engine":"ReplicatedMergeTree","parts":1204,"total_rows":98123456,"bytes_on_disk":1099511627776,"compressed_ratio":4.25,"last_modified":"2026-08-22 09:30:00"}"#,
        "\n",
        r#"{"database":"logs","name":"app","engine":null,"parts":431,"total_rows":22000,"bytes_on_disk":1024,"compressed_ratio":2.5,"last_modified":""}"#,
        "\n"
    );

    #[tokio::test]
    async fn tables_aggregate_tolerates_null_engine() {
        let (_s, c, seen) = server_with(vec![("ANY LEFT JOIN", TABLES_BODY)]).await;
        let rows = c.tables().await.expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].database, "events");
        assert_eq!(rows[0].name, "clicks");
        assert_eq!(rows[0].engine, "ReplicatedMergeTree");
        assert_eq!(rows[0].parts, 1204);
        assert_eq!(rows[0].bytes_on_disk, 1 << 40);
        assert_eq!(rows[0].compressed_ratio, 4.25);
        assert!(rows[0].last_modified.is_some());
        assert_eq!(rows[1].engine, "", "null -> default");
        assert_eq!(rows[1].last_modified, None);
        assert!(last_sql(&seen).contains("GROUP BY p.database, p.table"));
    }

    fn overview_routes() -> Vec<(&'static str, String)> {
        vec![
            ("uptime()", r#"{"running_queries":12,"active_merges":5,"databases_total":8,"tables_total":142,"parts_total":8931,"disk_used_bytes":549755813888,"uptime_seconds":1036800,"clickhouse_version":"25.3.1.1"}"#.to_string()),
            ("countIf(type = 'QueryStart')", r#"{"started_queries":462000}"#.to_string()),
            ("countIf(exception != '')", r#"{"slow_queries_24h":37,"failed_queries_24h":3}"#.to_string()),
            ("FROM system.replicas", r#"{"replicas_ok":3,"replicas_total":3}"#.to_string()),
            ("FROM system.disks", r#"{"v":1099511627776}"#.to_string()),
            ("toDate(event_time) = today()", r#"{"v":48210}"#.to_string()),
        ]
    }

    #[tokio::test]
    async fn overview_combines_all_subqueries() {
        let (_s, c, seen) = server_with(overview_routes()).await;
        let o = c.overview(TimeRange::SixHours).await.expect("overview");
        assert_eq!(o.running_queries, 12);
        assert_eq!(o.active_merges, 5);
        assert_eq!(o.databases_total, 8);
        assert_eq!(o.queries_today, 48_210);
        assert_eq!(o.tables_total, 142);
        assert_eq!(o.parts_total, 8931);
        assert_eq!(o.disk_used_bytes, 512 << 30);
        assert_eq!(o.disk_total_bytes, 1 << 40);
        assert_eq!(o.uptime_seconds, 86_400 * 12);
        assert_eq!(o.clickhouse_version, "25.3.1.1");
        assert_eq!(o.replicas_ok, 3);
        assert_eq!(o.replicas_total, 3);
        assert_eq!(o.slow_queries_24h, 37);
        assert_eq!(o.failed_queries_24h, 3);
        // 462000 queries over 6h => qps ~21.39
        assert!((o.qps - 462_000.0 / 21_600.0).abs() < 1e-9);
        let sent = seen.lock().expect("poisoned");
        assert!(sent.iter().any(|s| s.contains("FROM system.processes")));
        assert!(sent.iter().any(|s| s.contains("system.query_log")));
        assert!(sent.iter().any(|s| s.contains("toDateTime('")));
    }

    // --- error paths -------------------------------------------------------

    #[tokio::test]
    async fn non_200_becomes_query_error_with_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(500).set_body_string(
                    "DB::Exception: Syntax error: failed at position 12. CODE: 62",
                ),
            )
            .mount(&server)
            .await;
        let c = ClickHouseClient::new(server.uri(), "default", None);
        let err = c
            .query_rows::<raw::Scalar>(Q_RUNNING)
            .await
            .expect_err("must fail");
        match err {
            DataSourceError::Query { message } => {
                assert!(message.contains("500"), "{message}");
                assert!(message.contains("CODE: 62"), "{message}");
            }
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_row_is_query_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"v\":1}\n{oops\n"))
            .mount(&server)
            .await;
        let c = ClickHouseClient::new(server.uri(), "default", None);
        let err = c
            .query_rows::<raw::Scalar>("SELECT v")
            .await
            .expect_err("must fail");
        match err {
            DataSourceError::Query { message } => {
                assert!(message.contains("row decode"), "{message}")
            }
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn refused_connection_maps_to_connection_error() {
        // Reserve then drop a listener to claim a likely-free port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let c = ClickHouseClient::new(format!("http://127.0.0.1:{port}"), "default", None);
        let err = c.ping().await.expect_err("must fail");
        assert!(matches!(err, DataSourceError::Connection { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn ping_rejects_non_success_as_auth() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Not enough privileges"))
            .mount(&server)
            .await;
        let c = ClickHouseClient::new(server.uri(), "default", Some("pw".into()));
        let err = c.ping().await.expect_err("must fail");
        assert!(matches!(err, DataSourceError::Auth { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn ping_ok_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("1\n"))
            .mount(&server)
            .await;
        let c = ClickHouseClient::new(server.uri(), "default", None);
        assert!(c.ping().await.is_ok());
    }

    #[tokio::test]
    async fn sends_basic_auth_headers() {
        let (_s, c, seen) = server_with(vec![("FROM system.processes", RUNNING_BODY)]).await;
        let _ = c.running_queries().await;
        assert!(last_sql(&seen).contains("system.processes"));
    }

    fn last_sql(seen: &Mutex<Vec<String>>) -> String {
        seen.lock()
            .expect("poisoned")
            .last()
            .cloned()
            .unwrap_or_default()
    }
}
