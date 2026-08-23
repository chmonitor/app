//! Domain model shared by both connection modes. This is THE contract:
//! cloud API and direct ClickHouse clients deserialize into these types, the
//! UI renders them, and nothing else may leak mode-specific shapes upward.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

/// Global Tokio runtime backing every network future in this app.
///
/// gpui's executor is not a Tokio reactor: polling reqwest futures directly on
/// it panics inside hyper ("no reactor running"). Futures handed to gpui are
/// wrapped with [`tokio_block_on`] instead — they run on a real Tokio runtime,
/// and gpui only waits on the blocking call.
pub fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("global tokio runtime")
    })
}

/// Run `fut` to completion on the shared Tokio runtime.
pub fn tokio_block_on<F: std::future::Future>(fut: F) -> F::Output {
    global_runtime().block_on(fut)
}

#[derive(Debug, thiserror::Error)]
pub enum DataSourceError {
    #[error("connection failed: {message}")]
    Connection { message: String },
    #[error("authentication required or rejected: {message}")]
    Auth { message: String },
    #[error("query failed: {message}")]
    Query { message: String },
}

pub type Result<T> = std::result::Result<T, DataSourceError>;

/// Dashboard time ranges (matches web UI: 1h/6h/24h/7d/30d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeRange {
    OneHour,
    SixHours,
    TwentyFourHours,
    SevenDays,
    ThirtyDays,
}

impl TimeRange {
    pub fn duration(self) -> Duration {
        match self {
            Self::OneHour => Duration::hours(1),
            Self::SixHours => Duration::hours(6),
            Self::TwentyFourHours => Duration::hours(24),
            Self::SevenDays => Duration::days(7),
            Self::ThirtyDays => Duration::days(30),
        }
    }

    /// [from, to) window ending at `now`.
    pub fn window(self, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
        (now - self.duration(), now)
    }

    pub const ALL: [TimeRange; 5] = [
        Self::OneHour,
        Self::SixHours,
        Self::TwentyFourHours,
        Self::SevenDays,
        Self::ThirtyDays,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::OneHour => "1h",
            Self::SixHours => "6h",
            Self::TwentyFourHours => "24h",
            Self::SevenDays => "7d",
            Self::ThirtyDays => "30d",
        }
    }
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A point in a time series (epoch millis + value).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub t_ms: i64,
    pub v: f64,
}

/// Named series for traffic charts (queries/sent/received per bucket).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrafficSeries {
    pub queries_per_sec: Vec<SeriesPoint>,
    pub rows_read_per_sec: Vec<SeriesPoint>,
    pub network_rx_bps: Vec<SeriesPoint>,
    pub network_tx_bps: Vec<SeriesPoint>,
}

/// Headline numbers for the overview page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub qps: f64,
    pub running_queries: u64,
    pub slow_queries_24h: u64,
    pub failed_queries_24h: u64,
    pub active_merges: u64,
    pub replicas_ok: u64,
    pub replicas_total: u64,
    pub tables_total: u64,
    pub parts_total: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub uptime_seconds: u64,
    pub clickhouse_version: String,
}

/// One query row (list views for running/slow/failed).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QueryRow {
    pub id: String,
    pub user: String,
    pub elapsed_ms: f64,
    pub memory_bytes: u64,
    pub read_rows: u64,
    pub read_bytes: u64,
    pub exception: Option<String>,
    /// Normalized query shape when available, else raw SQL head.
    pub normalized_sql: String,
    pub started_at: Option<DateTime<Utc>>,
}

/// One merge/mutation in flight.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MergeRow {
    pub database: String,
    pub table: String,
    pub is_mutation: bool,
    pub progress: f32,
    pub num_parts: u64,
    pub total_memory_bytes: u64,
    pub elapsed_sec: f64,
}

/// Replica health row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplicaRow {
    pub database: String,
    pub table: String,
    pub replica_name: String,
    pub is_readonly: bool,
    pub is_session_expired: bool,
    pub absolute_delay_sec: f64,
    pub queue_size: u64,
    pub inserts_in_queue: u64,
    pub merges_in_queue: u64,
}

/// Health summary (web dashboard's health card).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
    pub readonly_tables: u64,
    pub replication_lag_max_sec: f64,
    pub zookeeper_available: bool,
    pub delayed_inserts: u64,
    pub distributed_files_to_insert: u64,
    pub background_pool_utilization: f32,
}

/// Table stats (tables page).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableStat {
    pub database: String,
    pub name: String,
    pub engine: String,
    pub parts: u64,
    pub rows: u64,
    pub bytes_on_disk: u64,
    pub compressed_ratio: f64,
    pub last_modified: Option<DateTime<Utc>>,
}

/// The one interface both modes implement. All methods are read-only and
/// cheap to call on a poll loop; implementors must be Send + Sync.
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Human-readable label for status bar, e.g. "cloud: acme.dash.chmonitor.dev".
    fn label(&self) -> String;

    /// Cheap connectivity/auth probe used by Connect screen and reconnects.
    async fn ping(&self) -> Result<()>;

    async fn overview(&self, range: TimeRange) -> Result<Overview>;
    async fn traffic(&self, range: TimeRange) -> Result<TrafficSeries>;
    async fn running_queries(&self) -> Result<Vec<QueryRow>>;
    async fn slow_queries(&self, range: TimeRange) -> Result<Vec<QueryRow>>;
    async fn failed_queries(&self, range: TimeRange) -> Result<Vec<QueryRow>>;
    async fn merges(&self) -> Result<Vec<MergeRow>>;
    async fn replicas(&self) -> Result<Vec<ReplicaRow>>;
    async fn health(&self) -> Result<Health>;
    async fn tables(&self) -> Result<Vec<TableStat>>;
}

/// Deterministic fixture data for tests and GUI smoke runs (`CHM_SMOKE=1`).
/// Lives here so every consumer shares identical smoke data.
pub struct MockDataSource {
    pub label: String,
}

impl MockDataSource {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

#[async_trait]
impl DataSource for MockDataSource {
    fn label(&self) -> String {
        self.label.clone()
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }

    async fn overview(&self, _range: TimeRange) -> Result<Overview> {
        let base = Overview {
            qps: 1284.5,
            running_queries: 12,
            slow_queries_24h: 37,
            failed_queries_24h: 3,
            active_merges: 5,
            replicas_ok: 3,
            replicas_total: 3,
            tables_total: 142,
            parts_total: 8931,
            disk_used_bytes: 512 * 1024 * 1024 * 1024,
            disk_total_bytes: 1024_u64 * 1024 * 1024 * 1024,
            uptime_seconds: 86_400 * 12,
            clickhouse_version: "25.3.1.1 (smoke)".into(),
        };
        // Vary by range so charts differ across selections in smoke shots.
        let mut o = base;
        if _range == TimeRange::SevenDays {
            o.qps *= 0.8;
        }
        Ok(o)
    }

    async fn traffic(&self, range: TimeRange) -> Result<TrafficSeries> {
        let buckets = 60;
        let (from, to) = range.window(Utc::now());
        let step = (to - from).num_milliseconds() as f64 / buckets as f64;
        let mut s = TrafficSeries::default();
        for i in 0..buckets {
            let x = i as f64;
            let wave = (x / 6.0).sin() * 0.4 + 1.0;
            let t = from + Duration::milliseconds((step * x) as i64);
            s.queries_per_sec.push(SeriesPoint {
                t_ms: t.timestamp_millis(),
                v: 1000.0 * wave,
            });
            s.rows_read_per_sec.push(SeriesPoint {
                t_ms: t.timestamp_millis(),
                v: 90_000.0 * wave,
            });
            s.network_rx_bps.push(SeriesPoint {
                t_ms: t.timestamp_millis(),
                v: 40_000_000.0 * wave,
            });
            s.network_tx_bps.push(SeriesPoint {
                t_ms: t.timestamp_millis(),
                v: 20_000_000.0 * wave,
            });
        }
        Ok(s)
    }

    async fn running_queries(&self) -> Result<Vec<QueryRow>> {
        Ok(vec![
            row_query(
                "q-1",
                "analytics",
                12_450.0,
                4 << 30,
                "SELECT count() FROM events GROUP BY user_id HAVING …",
            ),
            row_query(
                "q-2",
                "etl",
                3_120.5,
                800 << 20,
                "INSERT INTO reports SELECT …",
            ),
        ])
    }

    async fn slow_queries(&self, _range: TimeRange) -> Result<Vec<QueryRow>> {
        Ok(vec![
            row_query(
                "q-3",
                "analyst",
                45_000.0,
                12 << 30,
                "SELECT * FROM big_table WHERE lower(text) LIKE '%needle%'",
            ),
            row_query(
                "q-4",
                "dashboard",
                18_200.0,
                3 << 30,
                "SELECT date, count() FROM logs PREWHERE …",
            ),
        ])
    }

    async fn failed_queries(&self, _range: TimeRange) -> Result<Vec<QueryRow>> {
        Ok(vec![QueryRow {
            id: "q-5".into(),
            user: "svc-ingest".into(),
            elapsed_ms: 210.0,
            exception: Some("Table is readonly (replica delay)".into()),
            ..row_query(
                "q-5",
                "svc-ingest",
                210.0,
                0,
                "INSERT INTO ingest.events VALUES (…)",
            )
        }])
    }

    async fn merges(&self) -> Result<Vec<MergeRow>> {
        Ok(vec![
            MergeRow {
                database: "events".into(),
                table: "clicks".into(),
                is_mutation: false,
                progress: 0.62,
                num_parts: 148,
                total_memory_bytes: 900 << 20,
                elapsed_sec: 42.5,
            },
            MergeRow {
                database: "events".into(),
                table: "sessions".into(),
                is_mutation: true,
                progress: 0.11,
                num_parts: 1,
                total_memory_bytes: 120 << 20,
                elapsed_sec: 305.0,
            },
        ])
    }

    async fn replicas(&self) -> Result<Vec<ReplicaRow>> {
        Ok(vec![
            rep("ch-1", false, 0.0),
            rep("ch-2", false, 1.5),
            rep("ch-3", false, 0.0),
        ])
    }

    async fn health(&self) -> Result<Health> {
        Ok(Health {
            ok: true,
            readonly_tables: 0,
            replication_lag_max_sec: 1.5,
            zookeeper_available: true,
            delayed_inserts: 0,
            distributed_files_to_insert: 0,
            background_pool_utilization: 0.34,
        })
    }

    async fn tables(&self) -> Result<Vec<TableStat>> {
        Ok(vec![
            tbl(
                "events",
                "clicks",
                "ReplicatedMergeTree",
                1_204,
                98_000_000_000,
            ),
            tbl(
                "events",
                "sessions",
                "ReplicatedMergeTree",
                87,
                1_200_000_000,
            ),
            tbl("logs", "app", "MergeTree", 431, 22_000_000_000),
        ])
    }
}

fn row_query(id: &str, user: &str, elapsed_ms: f64, mem: u64, sql: &str) -> QueryRow {
    QueryRow {
        id: id.into(),
        user: user.into(),
        elapsed_ms,
        memory_bytes: mem,
        read_rows: 123_456_789,
        read_bytes: 456_789_123,
        exception: None,
        normalized_sql: sql.into(),
        started_at: Some(Utc::now() - Duration::milliseconds(elapsed_ms as i64)),
    }
}

fn rep(name: &str, ro: bool, delay: f64) -> ReplicaRow {
    ReplicaRow {
        replica_name: name.into(),
        is_readonly: ro,
        absolute_delay_sec: delay,
        queue_size: 0,
        ..Default::default()
    }
}

fn tbl(db: &str, name: &str, engine: &str, parts: u64, bytes: u64) -> TableStat {
    TableStat {
        database: db.into(),
        name: name.into(),
        engine: engine.into(),
        parts,
        rows: parts * 1_000_000,
        bytes_on_disk: bytes,
        compressed_ratio: 4.2,
        last_modified: Some(Utc::now() - Duration::hours(2)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::task::Poll;

    fn block_on<F: Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        loop {
            if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
                return out;
            }
        }
    }

    #[test]
    fn durations_cover_every_variant() {
        assert_eq!(
            TimeRange::ALL,
            [
                TimeRange::OneHour,
                TimeRange::SixHours,
                TimeRange::TwentyFourHours,
                TimeRange::SevenDays,
                TimeRange::ThirtyDays,
            ]
        );
        let expected = [
            (TimeRange::OneHour, Duration::hours(1)),
            (TimeRange::SixHours, Duration::hours(6)),
            (TimeRange::TwentyFourHours, Duration::hours(24)),
            (TimeRange::SevenDays, Duration::days(7)),
            (TimeRange::ThirtyDays, Duration::days(30)),
        ];
        for (range, want) in expected {
            assert_eq!(range.duration(), want);
        }
    }

    #[test]
    fn window_is_half_open_ending_at_now() {
        let now = Utc.with_ymd_and_hms(2026, 2, 14, 9, 30, 0).unwrap();
        for range in TimeRange::ALL {
            let (from, to) = range.window(now);
            assert_eq!(to, now);
            assert_eq!(from, now - range.duration());
            assert_eq!(to - from, range.duration());
        }
    }

    #[test]
    fn labels_and_display_agree() {
        let expected = [
            (TimeRange::OneHour, "1h"),
            (TimeRange::SixHours, "6h"),
            (TimeRange::TwentyFourHours, "24h"),
            (TimeRange::SevenDays, "7d"),
            (TimeRange::ThirtyDays, "30d"),
        ];
        for (range, want) in expected {
            assert_eq!(range.label(), want);
            assert_eq!(range.to_string(), want);
        }
    }

    #[test]
    fn mock_reports_label_and_pings() {
        let ds = MockDataSource::new("direct: local");
        assert_eq!(ds.label(), "direct: local");
        block_on(ds.ping()).unwrap();
    }

    #[test]
    fn overview_fixture_matches_expected_fields() {
        let ds = MockDataSource::new("m");
        for range in TimeRange::ALL {
            let o = block_on(ds.overview(range)).unwrap();
            assert_eq!(
                o.qps,
                if range == TimeRange::SevenDays {
                    1284.5 * 0.8
                } else {
                    1284.5
                }
            );
            assert_eq!(o.running_queries, 12);
            assert_eq!(o.slow_queries_24h, 37);
            assert_eq!(o.failed_queries_24h, 3);
            assert_eq!(o.active_merges, 5);
            assert_eq!(o.replicas_ok, 3);
            assert_eq!(o.replicas_total, 3);
            assert_eq!(o.tables_total, 142);
            assert_eq!(o.parts_total, 8931);
            assert_eq!(o.disk_used_bytes, 512 * 1024 * 1024 * 1024);
            assert_eq!(o.disk_total_bytes, 1024_u64 * 1024 * 1024 * 1024);
            assert_eq!(o.uptime_seconds, 86_400 * 12);
            assert_eq!(o.clickhouse_version, "25.3.1.1 (smoke)");
            assert!(o.disk_used_bytes <= o.disk_total_bytes);
        }
    }

    #[test]
    fn traffic_has_sixty_monotonic_buckets_in_each_series() {
        let ds = MockDataSource::new("m");
        for range in TimeRange::ALL {
            let before = Utc::now().timestamp_millis();
            let s = block_on(ds.traffic(range)).unwrap();
            let after = Utc::now().timestamp_millis();

            let series = [
                ("queries_per_sec", &s.queries_per_sec),
                ("rows_read_per_sec", &s.rows_read_per_sec),
                ("network_rx_bps", &s.network_rx_bps),
                ("network_tx_bps", &s.network_tx_bps),
            ];
            for (name, pts) in series {
                assert_eq!(pts.len(), 60, "{name} bucket count for {range}");
                assert!(
                    pts.windows(2).all(|w| w[0].t_ms < w[1].t_ms),
                    "{name} timestamps not strictly increasing for {range}"
                );
                let lower = before - range.duration().num_milliseconds();
                let upper = after - range.duration().num_milliseconds();
                assert!(
                    pts[0].t_ms >= lower && pts[0].t_ms <= upper,
                    "{name} first bucket outside [{lower}, {upper}] for {range}",
                );
                assert!(
                    pts.last().unwrap().t_ms <= after,
                    "{name} buckets reach past now for {range}"
                );
                assert!(
                    pts.iter().all(|p| p.v.is_finite() && p.v > 0.0),
                    "{name} has non-positive or non-finite values for {range}"
                );
            }
        }
    }

    #[test]
    fn running_and_slow_query_rows_have_complete_shape() {
        let ds = MockDataSource::new("m");
        let running = block_on(ds.running_queries()).unwrap();
        let slow = block_on(ds.slow_queries(TimeRange::TwentyFourHours)).unwrap();

        assert_eq!(running.len(), 2);
        assert_eq!(running[0].id, "q-1");
        assert_eq!(running[0].user, "analytics");
        assert_eq!(
            running[0].normalized_sql,
            "SELECT count() FROM events GROUP BY user_id HAVING …"
        );
        assert_eq!(running[1].id, "q-2");
        assert_eq!(running[1].user, "etl");

        assert_eq!(slow.len(), 2);
        assert_eq!(slow[0].id, "q-3");
        assert_eq!(slow[0].user, "analyst");
        assert_eq!(slow[1].id, "q-4");
        assert_eq!(slow[1].user, "dashboard");

        for row in running.iter().chain(slow.iter()) {
            assert!(!row.id.is_empty());
            assert!(!row.user.is_empty());
            assert!(row.elapsed_ms > 0.0);
            assert!(row.memory_bytes > 0);
            assert!(row.read_rows > 0);
            assert!(row.read_bytes > 0);
            assert!(!row.normalized_sql.is_empty());
            assert!(row.started_at.is_some());
            assert!(row.exception.is_none());
        }
    }

    #[test]
    fn failed_query_rows_carry_an_exception() {
        let ds = MockDataSource::new("m");
        let failed = block_on(ds.failed_queries(TimeRange::TwentyFourHours)).unwrap();
        assert_eq!(failed.len(), 1);
        let row = &failed[0];
        assert_eq!(row.id, "q-5");
        assert_eq!(row.user, "svc-ingest");
        assert_eq!(row.elapsed_ms, 210.0);
        assert_eq!(
            row.exception.as_deref(),
            Some("Table is readonly (replica delay)")
        );
        assert!(row.started_at.is_some());
    }

    #[test]
    fn merge_replica_health_and_table_fixtures_hold() {
        let ds = MockDataSource::new("m");

        let merges = block_on(ds.merges()).unwrap();
        assert_eq!(merges.len(), 2);
        assert!(!merges[0].is_mutation);
        assert!(merges[1].is_mutation);
        assert!(merges.iter().all(|m| (0.0..=1.0).contains(&m.progress)));
        assert!(
            merges
                .iter()
                .all(|m| !m.database.is_empty() && !m.table.is_empty())
        );

        let replicas = block_on(ds.replicas()).unwrap();
        assert_eq!(replicas.len(), 3);
        assert!(
            replicas
                .iter()
                .all(|r| !r.is_readonly && !r.is_session_expired)
        );
        assert_eq!(replicas[1].replica_name, "ch-2");
        assert_eq!(replicas[1].absolute_delay_sec, 1.5);

        let health = block_on(ds.health()).unwrap();
        assert!(health.ok);
        assert!(health.zookeeper_available);
        assert_eq!(health.readonly_tables, 0);
        assert_eq!(health.delayed_inserts, 0);
        assert_eq!(health.distributed_files_to_insert, 0);

        let tables = block_on(ds.tables()).unwrap();
        assert_eq!(tables.len(), 3);
        assert!(
            tables
                .iter()
                .all(|t| !t.database.is_empty() && !t.name.is_empty() && !t.engine.is_empty())
        );
        assert!(tables.iter().all(|t| t.rows == t.parts * 1_000_000));
        assert!(
            tables
                .iter()
                .all(|t| t.bytes_on_disk > 0 && t.last_modified.is_some())
        );
    }

    #[test]
    fn boxed_dyn_data_source_dispatches() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Box<dyn DataSource>>();

        let ds: Box<dyn DataSource> = Box::new(MockDataSource::new("boxed"));
        assert_eq!(ds.label(), "boxed");
        block_on(ds.ping()).unwrap();
        let o = block_on(ds.overview(TimeRange::SixHours)).unwrap();
        assert_eq!(o.qps, 1284.5);
        assert_eq!(o.running_queries, 12);
        let t = block_on(ds.traffic(TimeRange::OneHour)).unwrap();
        assert_eq!(t.queries_per_sec.len(), 60);
        let h = block_on(ds.health()).unwrap();
        assert!(h.ok);
    }
}
