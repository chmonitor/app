# chmonitor Desktop

GPUI desktop client for [chmonitor](https://chmonitor.dev) — ClickHouse
monitoring for macOS and Linux, with two connection modes:

1. **Cloud / dashboard endpoint** — talks to `dash.chmonitor.dev` or any
   self-hosted chmonitor worker (`chm-cloud-api`).
2. **Direct ClickHouse** — speaks to your ClickHouse instance over HTTP
   (`chm-clickhouse`), SQL ported from the web dashboard.
3. **Postgres** — read-only `pg_stat_*` monitoring (`chm-postgres`), same
   host switcher as ClickHouse. Merges/Traffic pages hide on a PG host.

## Build

```sh
cargo build -p chm-app            # debug
cargo build --release -p chm-app  # release (LTO, stripped)
```

### macOS

GPUI paints with Metal. Full Xcode ships the `metal` compiler used to
precompile shaders at build time. This workspace enables
`gpui_platform/runtime_shaders` so a Mac with only Command Line Tools
(`xcode-select -p` → `/Library/Developer/CommandLineTools`) can still
`cargo build -p chm-app`; shaders compile on first launch instead.

To precompile shaders (faster startup) once Xcode is installed:

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
xcodebuild -downloadComponent MetalToolchain   # Xcode 26+
```

Then drop `runtime_shaders` from the `gpui_platform` features in the
workspace `Cargo.toml`.

## Run

```sh
cargo run -p chm-app -- --connect          # open Connect screen
cargo run -p chm-app -- --help
CHM_SMOKE=1 cargo run -p chm-app           # built-in fixture data, no network
CHM_PROFILE=work cargo run -p chm-app      # named saved profile
CHM_CONFIG=/tmp/chmonitor.toml cargo run -p chm-app
```

Named profiles live under `[profiles.<name>]` in `config.toml`; the
default connection is `[profile]`. `r` refreshes the current page;
keys `1`–`8` switch sidebar destinations; `cmd-b` toggles the sidebar;
`cmd-,` opens Settings. The sidebar host switcher lists `[profile]` plus
`[profiles.<name>]`; Connect's optional Name field saves a named host.

## Layout

| Path | Purpose |
|---|---|
| `crates/chm-core` | domain types + `DataSource` trait + mock data |
| `crates/chm-cloud-api` | mode 1: dashboard REST client |
| `crates/chm-clickhouse` | mode 2: direct ClickHouse HTTP client |
| `crates/chm-postgres` | mode 3: direct Postgres (`pg_stat_*`) |
| `crates/chm-update` | channel-aware update checker (stable/beta) |
| `crates/chm-telemetry` | opt-in telemetry + perf metrics |
| `app/` | GPUI UI: [gpui-base](https://longbridge.github.io/gpui-component/base/getting-started.md) primitives (buttons, radios, tables) + [gpui-component](https://longbridge.github.io/gpui-component/) for sidebar, charts, theme |
| `.github/workflows/` | CI: lint, test, build matrix, releases |

## Testing

```sh
cargo test --workspace        # unit + wiremock + SQL snapshots
scripts/smoke.sh              # GUI smoke on Linux desktop (display :1)
scripts/smoke-mac.sh          # GUI smoke on macOS (CHM_SMOKE=1 + screenshot)
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

