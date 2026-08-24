//! Direct Postgres client for monitored sources (`pg_stat_activity` /
//! `pg_stat_statements` / `pg_stat_user_tables`). Read-only: every session
//! pins `default_transaction_read_only=on`. SQL follows chmonitor's
//! postgres-source collectors.

use async_trait::async_trait;
use chm_core::{
    DataSource, DataSourceError, Health, MergeRow, Overview, QueryRow, ReplicaRow, Result,
    SourceEngine, TableStat, TimeRange, TrafficSeries,
};
use chrono::{TimeZone, Utc};
use tokio_postgres::{Client, Config, NoTls, Row};

/// Direct Postgres source; implements [`chm_core::DataSource`].
#[derive(Debug, Clone)]
pub struct PostgresClient {
    config: Config,
    sslmode: SslMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SslMode {
    Disable,
    Prefer,
    Require,
}

impl PostgresClient {
    pub fn new(
        url: impl AsRef<str>,
        user: Option<String>,
        password: Option<String>,
        database: Option<String>,
        sslmode: Option<String>,
    ) -> Result<Self> {
        let (mut config, inferred_ssl) = parse_endpoint(url.as_ref())?;
        if let Some(u) = user.filter(|u| !u.is_empty()) {
            config.user(&u);
        }
        if let Some(p) = password.filter(|p| !p.is_empty()) {
            config.password(p);
        }
        if let Some(d) = database.filter(|d| !d.is_empty()) {
            config.dbname(&d);
        }
        if config.get_user().is_none() {
            config.user("postgres");
        }
        if config.get_dbname().is_none() {
            config.dbname("postgres");
        }
        let sslmode = sslmode
            .as_deref()
            .map(parse_sslmode)
            .or(inferred_ssl)
            .unwrap_or(SslMode::Prefer);
        Ok(Self { config, sslmode })
    }

    async fn with_client<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        match self.sslmode {
            SslMode::Disable | SslMode::Prefer => self.connect_plain(f).await,
            SslMode::Require => self.connect_tls(f).await,
        }
    }

    async fn connect_plain<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let (client, conn) = self.config.connect(NoTls).await.map_err(map_pg_err)?;
        let handle = tokio::spawn(async move {
            let _ = conn.await;
        });
        pin_read_only(&client).await?;
        let out = f(client).await;
        handle.abort();
        out
    }

    async fn connect_tls<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let connector = native_tls::TlsConnector::builder().build().map_err(|e| {
            DataSourceError::Connection {
                message: format!("tls connector: {e}"),
            }
        })?;
        let connector = postgres_native_tls::MakeTlsConnector::new(connector);
        let (client, conn) = self.config.connect(connector).await.map_err(map_pg_err)?;
        let handle = tokio::spawn(async move {
            let _ = conn.await;
        });
        pin_read_only(&client).await?;
        let out = f(client).await;
        handle.abort();
        out
    }
}

async fn pin_read_only(client: &Client) -> Result<()> {
    client
        .batch_execute("SET default_transaction_read_only = on")
        .await
        .map_err(map_pg_err)
}

fn map_pg_err(e: tokio_postgres::Error) -> DataSourceError {
    let message = e.to_string();
    let code = e.code().map(|c| c.code());
    match code {
        Some("28P01" | "28000") => DataSourceError::Auth { message },
        Some("3D000") => DataSourceError::Query { message },
        _ if e.is_closed() => DataSourceError::Connection { message },
        _ => DataSourceError::Query { message },
    }
}

fn parse_sslmode(s: &str) -> SslMode {
    match s.to_ascii_lowercase().as_str() {
        "disable" | "allow" => SslMode::Disable,
        "require" | "verify-ca" | "verify-full" => SslMode::Require,
        _ => SslMode::Prefer,
    }
}

/// `postgres://…` URLs, or `host:port` / `host`.
fn parse_endpoint(raw: &str) -> Result<(Config, Option<SslMode>)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(DataSourceError::Connection {
            message: "empty postgres url".into(),
        });
    }
    if raw.contains("://") {
        let ssl = raw
            .split(['?', '&'])
            .find_map(|p| p.strip_prefix("sslmode="))
            .map(parse_sslmode);
        let cfg: Config =
            raw.parse()
                .map_err(|e: tokio_postgres::Error| DataSourceError::Connection {
                    message: e.to_string(),
                })?;
        return Ok((cfg, ssl));
    }
    let mut cfg = Config::new();
    match raw.rsplit_once(':') {
        Some((host, port)) if port.parse::<u16>().is_ok() => {
            cfg.host(host);
            cfg.port(port.parse().unwrap());
        }
        _ => {
            cfg.host(raw);
            cfg.port(5432);
        }
    }
    Ok((cfg, None))
}

#[async_trait]
impl DataSource for PostgresClient {
    fn label(&self) -> String {
        use tokio_postgres::config::Host;
        let host = match self.config.get_hosts().first() {
            Some(Host::Tcp(h)) => h.clone(),
            _ => "postgres".into(),
        };
        format!("postgres: {host}")
    }

    fn engine(&self) -> SourceEngine {
        SourceEngine::Postgres
    }

    async fn ping(&self) -> Result<()> {
        self.with_client(|client| async move {
            client
                .query_one("SELECT 1", &[])
                .await
                .map_err(map_pg_err)?;
            Ok(())
        })
        .await
    }

    async fn overview(&self, _range: TimeRange) -> Result<Overview> {
        self.with_client(|client| async move {
            let row = client
                .query_one(
                    r#"
SELECT
  (SELECT count(*)::bigint FROM pg_stat_activity
    WHERE state = 'active' AND pid <> pg_backend_pid()) AS running_queries,
  (SELECT count(*)::bigint FROM pg_stat_user_tables) AS tables_total,
  (SELECT coalesce(sum(pg_database_size(oid)), 0)::bigint FROM pg_database) AS disk_used_bytes,
  extract(epoch FROM (now() - pg_postmaster_start_time()))::bigint AS uptime_seconds,
  version() AS server_version,
  (SELECT count(*)::bigint FROM pg_stat_replication) AS replicas_total,
  (SELECT count(*)::bigint FROM pg_stat_replication WHERE state = 'streaming') AS replicas_ok,
  coalesce((
    SELECT sum(xact_commit + xact_rollback)
           / greatest(extract(epoch FROM (now() - stats_reset)), 1)
    FROM pg_stat_database
  ), 0)::float8 AS qps
"#,
                    &[],
                )
                .await
                .map_err(map_pg_err)?;
            let running = get_i64(&row, "running_queries").max(0) as u64;
            let tables = get_i64(&row, "tables_total").max(0) as u64;
            let disk = get_i64(&row, "disk_used_bytes").max(0) as u64;
            let uptime = get_i64(&row, "uptime_seconds").max(0) as u64;
            let version: String = row.try_get("server_version").unwrap_or_default();
            let replicas_total = get_i64(&row, "replicas_total").max(0) as u64;
            let replicas_ok = get_i64(&row, "replicas_ok").max(0) as u64;
            let qps: f64 = row.try_get("qps").unwrap_or(0.0);
            Ok(Overview {
                qps,
                running_queries: running,
                tables_total: tables,
                disk_used_bytes: disk,
                disk_total_bytes: disk,
                uptime_seconds: uptime,
                clickhouse_version: version,
                replicas_total,
                replicas_ok,
                ..Overview::default()
            })
        })
        .await
    }

    async fn traffic(&self, _range: TimeRange) -> Result<TrafficSeries> {
        Ok(TrafficSeries::default())
    }

    async fn running_queries(&self) -> Result<Vec<QueryRow>> {
        self.with_client(|client| async move {
            let rows = client
                .query(
                    r#"
SELECT pid::text AS id,
       coalesce(usename, '') AS user_name,
       coalesce(extract(epoch FROM (now() - query_start)), 0) * 1000 AS elapsed_ms,
       left(coalesce(query, ''), 240) AS sql,
       query_start
FROM pg_stat_activity
WHERE state = 'active' AND pid <> pg_backend_pid()
ORDER BY query_start NULLS LAST
LIMIT 100
"#,
                    &[],
                )
                .await
                .map_err(map_pg_err)?;
            Ok(rows.iter().map(activity_row).collect())
        })
        .await
    }

    async fn slow_queries(&self, _range: TimeRange) -> Result<Vec<QueryRow>> {
        self.with_client(|client| async move {
            let rows = match client
                .query(
                    r#"
SELECT queryid::text AS id,
       '' AS user_name,
       mean_exec_time AS elapsed_ms,
       left(query, 240) AS sql
FROM pg_stat_statements
ORDER BY mean_exec_time DESC
LIMIT 100
"#,
                    &[],
                )
                .await
            {
                Ok(rows) => rows,
                Err(_) => return Ok(Vec::new()),
            };
            Ok(rows
                .iter()
                .map(|r| QueryRow {
                    id: r.try_get("id").unwrap_or_default(),
                    user: r.try_get("user_name").unwrap_or_default(),
                    elapsed_ms: r.try_get("elapsed_ms").unwrap_or(0.0),
                    normalized_sql: r.try_get("sql").unwrap_or_default(),
                    ..QueryRow::default()
                })
                .collect())
        })
        .await
    }

    async fn failed_queries(&self, _range: TimeRange) -> Result<Vec<QueryRow>> {
        Ok(Vec::new())
    }

    async fn merges(&self) -> Result<Vec<MergeRow>> {
        Ok(Vec::new())
    }

    async fn replicas(&self) -> Result<Vec<ReplicaRow>> {
        self.with_client(|client| async move {
            let rows = client
                .query(
                    r#"
SELECT coalesce(nullif(application_name, ''), client_addr::text, pid::text) AS replica_name,
       coalesce(state, '') AS state,
       coalesce(extract(epoch FROM replay_lag), 0)::float8 AS delay
FROM pg_stat_replication
"#,
                    &[],
                )
                .await
                .map_err(map_pg_err)?;
            Ok(rows
                .iter()
                .map(|r| {
                    let state: String = r.try_get("state").unwrap_or_default();
                    ReplicaRow {
                        replica_name: r.try_get("replica_name").unwrap_or_default(),
                        absolute_delay_sec: r.try_get("delay").unwrap_or(0.0),
                        is_readonly: state != "streaming",
                        ..ReplicaRow::default()
                    }
                })
                .collect())
        })
        .await
    }

    async fn health(&self) -> Result<Health> {
        self.with_client(|client| async move {
            let row = client
                .query_one(
                    r#"
SELECT
  (SELECT count(*)::bigint FROM pg_stat_activity) AS conns,
  (SELECT setting::bigint FROM pg_settings WHERE name = 'max_connections') AS max_conns,
  pg_is_in_recovery() AS in_recovery,
  (SELECT count(*)::bigint FROM pg_stat_activity WHERE state = 'idle in transaction') AS idle_txn
"#,
                    &[],
                )
                .await
                .map_err(map_pg_err)?;
            let conns = get_i64(&row, "conns").max(0) as f32;
            let max = get_i64(&row, "max_conns").max(1) as f32;
            let in_recovery: bool = row.try_get("in_recovery").unwrap_or(false);
            let idle: u64 = get_i64(&row, "idle_txn").max(0) as u64;
            let util = (conns / max).clamp(0.0, 1.0);
            Ok(Health {
                ok: !in_recovery && util < 0.9,
                zookeeper_available: true,
                delayed_inserts: idle,
                background_pool_utilization: util,
                ..Health::default()
            })
        })
        .await
    }

    async fn tables(&self) -> Result<Vec<TableStat>> {
        self.with_client(|client| async move {
            let rows = client
                .query(
                    r#"
SELECT schemaname AS database,
       relname AS name,
       'heap' AS engine,
       n_live_tup::bigint AS rows,
       pg_total_relation_size(relid)::bigint AS bytes_on_disk
FROM pg_stat_user_tables
ORDER BY pg_total_relation_size(relid) DESC
LIMIT 200
"#,
                    &[],
                )
                .await
                .map_err(map_pg_err)?;
            Ok(rows
                .iter()
                .map(|r| TableStat {
                    database: r.try_get("database").unwrap_or_default(),
                    name: r.try_get("name").unwrap_or_default(),
                    engine: r.try_get("engine").unwrap_or_default(),
                    rows: get_i64(r, "rows").max(0) as u64,
                    bytes_on_disk: get_i64(r, "bytes_on_disk").max(0) as u64,
                    ..TableStat::default()
                })
                .collect())
        })
        .await
    }
}

fn activity_row(r: &Row) -> QueryRow {
    let started_at = r
        .try_get::<_, Option<chrono::DateTime<Utc>>>("query_start")
        .ok()
        .flatten()
        .or_else(|| {
            r.try_get::<_, Option<chrono::NaiveDateTime>>("query_start")
                .ok()
                .flatten()
                .map(|n| Utc.from_utc_datetime(&n))
        });
    QueryRow {
        id: r.try_get("id").unwrap_or_default(),
        user: r.try_get("user_name").unwrap_or_default(),
        elapsed_ms: r.try_get("elapsed_ms").unwrap_or(0.0),
        normalized_sql: r.try_get("sql").unwrap_or_default(),
        started_at,
        ..QueryRow::default()
    }
}

fn get_i64(row: &Row, col: &str) -> i64 {
    row.try_get::<_, i64>(col)
        .or_else(|_| row.try_get::<_, i32>(col).map(|v| v as i64))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_port_and_url() {
        let (cfg, ssl) = parse_endpoint("localhost:5432").unwrap();
        assert!(ssl.is_none());
        assert_eq!(cfg.get_ports(), [5432]);

        let (cfg, ssl) = parse_endpoint("postgres://alice@db.example:5433/app").unwrap();
        assert_eq!(cfg.get_user(), Some("alice"));
        assert_eq!(cfg.get_dbname(), Some("app"));
        assert_eq!(cfg.get_ports(), [5433]);
        assert!(ssl.is_none());

        let (_, ssl) = parse_endpoint("postgres://h/db?sslmode=disable").unwrap();
        assert_eq!(ssl, Some(SslMode::Disable));
    }

    #[test]
    fn parse_empty_fails() {
        assert!(parse_endpoint("").is_err());
        assert!(parse_endpoint("   ").is_err());
    }

    #[test]
    fn sslmode_mapping() {
        assert_eq!(parse_sslmode("disable"), SslMode::Disable);
        assert_eq!(parse_sslmode("require"), SslMode::Require);
        assert_eq!(parse_sslmode("prefer"), SslMode::Prefer);
    }
}
