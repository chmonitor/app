# chmonitor Desktop

GPUI + bezel desktop client for [chmonitor](https://chmonitor.dev) — ClickHouse
monitoring for macOS and Linux, with two connection modes:

1. **Cloud / dashboard endpoint** — talks to `dash.chmonitor.dev` or any
   self-hosted chmonitor worker (`chm-cloud-api`).
2. **Direct ClickHouse** — speaks to your ClickHouse instance over HTTP
   (`chm-clickhouse`), SQL ported from the web dashboard.

## Build

```sh
cargo build -p chm-app            # debug
cargo build --release -p chm-app  # release (LTO, stripped)
```

## Run

```sh
cargo run -p chm-app -- --connect          # open Connect screen
CHM_SMOKE=1 cargo run -p chm-app           # built-in fixture data, no network
CHM_PROFILE=work cargo run -p chm-app      # named saved profile
```

## Layout

| Path | Purpose |
|---|---|
| `crates/chm-core` | domain types + `DataSource` trait + mock data |
| `crates/chm-cloud-api` | mode 1: dashboard REST client |
| `crates/chm-clickhouse` | mode 2: direct ClickHouse HTTP client |
| `crates/chm-update` | channel-aware update checker (stable/beta) |
| `crates/chm-telemetry` | opt-in telemetry + perf metrics |
| `app/` | GPUI + bezel UI |
| `.github/workflows/` | CI: lint, test, build matrix, releases |

## Testing

```sh
cargo test --workspace        # unit + wiremock + SQL snapshots
scripts/smoke.sh              # GUI smoke on Linux desktop (display :1)
```

CI runs lint (`fmt` + `clippy -D warnings`), the workspace tests, a
cross-platform build matrix, and an end-to-end GUI smoke job under Xvfb on
every pull request and push to `main`.

## Channels & releases

Releases stay within **v0.1.x** while the app stabilizes; out-of-range tags
are rejected by the release pipeline.

- `stable` — tagged releases via release-please.
- `beta` — pre-release builds (tag suffix `-beta.N`); the in-app update
  checker follows the channel baked into the profile.
- Auto-update: in-app check + download prompt (chm-update); update manifests
  are emitted per release and attached alongside signed archives.

