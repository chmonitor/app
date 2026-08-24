//! App shell — window layout, sidebar nav, page routing, status bar,
//! 30-second poll loop and the startup update-check hook.
//! AGENT D OWNS THIS FILE.
//!
//! All gpui types come through `bezel::gpui`; widgets come from the bezel
//! facade (`bezel::theme`, `bezel::ui`).

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chm_core::{
    DataSource, Health, MergeRow, MockDataSource, Overview, QueryRow, ReplicaRow, SourceEngine,
    TableStat, TimeRange, TrafficSeries,
};

use bezel::gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, FocusHandle, Focusable, Hsla, KeyBinding,
    KeyDownEvent, Render, SharedString, WeakEntity, Window, actions, div, prelude::*, px,
};
use bezel::theme::Theme;
use bezel::ui::widgets::status_dot;

use crate::config::{
    ConfigFile, Host, ProfileConfig, active_host_id, cli, config_path, list_hosts, load_config,
    load_profile, profile_for_host, save_config, source_from_profile,
};
use crate::connect::{ConnectEvent, ConnectFlow};
use crate::pages::Page;
use crate::pages::health::HealthPage;
use crate::pages::merges::MergesPage;
use crate::pages::overview::OverviewPage;
use crate::pages::queries::QueriesPage;
use crate::pages::replicas::ReplicasPage;
use crate::pages::settings::SettingsPage;
use crate::pages::tables::TablesPage;
use crate::pages::traffic::TrafficPage;

actions!(chm_shell, [Refresh, ToggleSidebar, OpenSettings]);

/// Seconds between automatic background refreshes.
const POLL_SECS: u64 = 30;
/// Viewport width below which the sidebar collapses to an icon strip.
const COMPACT_BELOW: f32 = 900.0;
/// Sidebar width expanded / collapsed.
const SIDEBAR_W: f32 = 190.0;
const SIDEBAR_W_COMPACT: f32 = 48.0;

/// Perf metrics live for the whole process; recording is gated by
/// `[telemetry] enabled=true` in config.toml (never on by default).
fn perf() -> &'static chm_telemetry::PerfMetrics {
    static PERF: OnceLock<chm_telemetry::PerfMetrics> = OnceLock::new();
    PERF.get_or_init(chm_telemetry::PerfMetrics::new)
}

// ---------------------------------------------------------------------------
// Shell
// ---------------------------------------------------------------------------

/// Connection state shown in the status bar dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connected,
    Connecting,
    Error,
}

impl ConnState {
    fn dot(self, theme: &Theme) -> Hsla {
        match self {
            Self::Connected => theme.success,
            Self::Connecting => theme.warning,
            Self::Error => theme.danger,
        }
    }
}

/// Result of the one-shot startup update check.
#[derive(Debug, Clone)]
struct UpdateNote(SharedString);

/// Glanceable facts about the active host (status bar).
#[derive(Debug, Clone, Default)]
struct HostStatus {
    version: Option<String>,
    replicas_ok: u64,
    replicas_total: u64,
    health_ok: Option<bool>,
    fetch_ms: Option<f64>,
}

/// The root view: owns routing, the active data source and the poll task.
pub struct Shell {
    focus: FocusHandle,
    page: Page,
    range: TimeRange,
    source: Option<Arc<Box<dyn DataSource>>>,
    conn: ConnState,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
    last_error: Option<String>,
    overview: Entity<OverviewPage>,
    queries: Entity<QueriesPage>,
    merges: Entity<MergesPage>,
    replicas: Entity<ReplicasPage>,
    health: Entity<HealthPage>,
    tables: Entity<TablesPage>,
    traffic: Entity<TrafficPage>,
    connect: Entity<ConnectFlow>,
    settings: Entity<SettingsPage>,
    update_note: Option<UpdateNote>,
    /// `None` follows the viewport; `Some` is a click/`cmd-b` override.
    sidebar_collapsed: Option<bool>,
    active_host: Option<String>,
    host_menu_open: bool,
    host_status: HostStatus,
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    /// Mode selection per contract:
    /// * `CHM_SMOKE=1` forces [`MockDataSource`];
    /// * else the saved profile builds a cloud/direct client;
    /// * else no source — Connect becomes the content pane.
    #[allow(clippy::type_complexity)]
    fn pick_source() -> (Option<Arc<Box<dyn DataSource>>>, ConnState, Option<String>) {
        if std::env::var("CHM_SMOKE").is_ok() {
            return (
                Some(Arc::new(
                    Box::new(MockDataSource::new("mock (CHM_SMOKE)")) as Box<dyn DataSource>
                )),
                ConnState::Connected,
                Some("smoke".into()),
            );
        }
        let cfg = load_config();
        let id = active_host_id(&cfg);
        match id
            .as_deref()
            .and_then(|id| profile_for_host(&cfg, id))
            .and_then(|p| source_from_profile(&p))
        {
            Some(src) => (Some(Arc::new(src)), ConnState::Connecting, id),
            None => (None, ConnState::Error, id),
        }
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let (source, conn, active_host) = Self::pick_source();

        // Telemetry hook: opt-in only. Recording stays off unless the user
        // explicitly set `[telemetry] enabled = true`; nothing else enables it.
        let telemetry_enabled = config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| toml::from_str::<ConfigFile>(&text).ok())
            .map(|cfg| cfg.telemetry.enabled)
            .unwrap_or(false);
        if telemetry_enabled {
            // Config built but nothing transmitted: PerfMetrics only records
            // local latency numbers, and recording itself stays gated here.
            let _cfg = chm_telemetry::TelemetryConfig::default().set_enabled(true);
        }

        // One-shot update check, background thread. Failures are silent unless
        // CHM_UPDATE_URL overrides the manifest base (chm_update semantics).
        let channel = match load_profile().and_then(|p| p.channel).as_deref() {
            Some("beta") => chm_update::Channel::Beta,
            _ => chm_update::Channel::Stable,
        };
        cx.spawn(async move |this, cx| {
            let checker = chm_update::UpdateChecker::production();
            let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .unwrap_or_else(|_| semver::Version::new(0, 1, 1));
            let note = match chm_core::tokio_block_on(checker.check(channel, &current)) {
                Ok(Some(release)) => {
                    UpdateNote(format!("update available: v{}", release.version()).into())
                }
                Ok(None) => UpdateNote("up to date".into()),
                // Keep the status bar from sitting on "update check…" forever
                // when the manifest host is unreachable.
                Err(_) => UpdateNote(SharedString::default()),
            };
            let _ = this.update(cx, |shell, cx| {
                shell.update_note = Some(note);
                cx.notify();
            });
        })
        .detach();

        let force_connect = cli().connect;
        let page = if force_connect || source.is_none() {
            Page::Connect
        } else {
            Page::Overview
        };

        let mut shell = Self {
            focus: cx.focus_handle(),
            page,
            range: TimeRange::TwentyFourHours,
            source,
            conn,
            last_refresh: None,
            last_error: None,
            overview: cx.new(|_| OverviewPage::new()),
            queries: cx.new(|_| QueriesPage::new()),
            merges: cx.new(|_| MergesPage::new()),
            replicas: cx.new(|_| ReplicasPage::new()),
            health: cx.new(|_| HealthPage::new()),
            tables: cx.new(|_| TablesPage::new()),
            traffic: cx.new(|_| TrafficPage::new()),
            connect: cx.new(|cx| ConnectFlow::new(load_profile(), cx)),
            settings: cx.new(|_| SettingsPage::new()),
            update_note: None,
            sidebar_collapsed: None,
            active_host,
            host_menu_open: false,
            host_status: HostStatus::default(),
        };

        // Digits 1-8 switch pages; handled in render's on_key_down so it works
        // wherever focus sits in this view's subtree. `r` is an action.
        cx.bind_keys([
            KeyBinding::new("r", Refresh, None),
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
        ]);

        // Rebuild the source after the Connect screen writes a new profile.
        cx.subscribe(&shell.connect, |this, _, event: &ConnectEvent, cx| {
            let ConnectEvent::SavedProfile { profile, host_id } = event;
            this.apply_host(host_id.clone(), profile.clone());
            this.page = Page::Overview;
            this.refresh_now(cx);
            cx.notify();
        })
        .detach();

        shell.start_poll(cx);
        shell.refresh_now(cx);
        shell
    }

    fn goto(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }
        self.page = page;
        self.refresh_now(cx);
        cx.notify();
    }

    fn toggle_sidebar(&mut self, narrow: bool, cx: &mut Context<Self>) {
        let compact = sidebar_is_compact(self.sidebar_collapsed, narrow);
        self.sidebar_collapsed = Some(!compact);
        cx.notify();
    }

    fn apply_host(&mut self, host_id: String, profile: ProfileConfig) {
        self.active_host = Some(host_id);
        self.source = source_from_profile(&profile).map(Arc::new);
        self.conn = if self.source.is_some() {
            ConnState::Connecting
        } else {
            ConnState::Error
        };
        self.host_status = HostStatus::default();
        self.host_menu_open = false;
        self.last_error = None;
    }

    fn switch_host(&mut self, host_id: String, cx: &mut Context<Self>) {
        if self.active_host.as_deref() == Some(host_id.as_str()) {
            self.host_menu_open = false;
            cx.notify();
            return;
        }
        if std::env::var("CHM_SMOKE").is_ok() {
            self.host_menu_open = false;
            cx.notify();
            return;
        }
        let cfg = load_config();
        let Some(profile) = profile_for_host(&cfg, &host_id) else {
            return;
        };
        let mut cfg = load_config();
        cfg.ui.host = Some(host_id.clone());
        let _ = save_config(&cfg);
        self.apply_host(host_id, profile);
        if matches!(self.page, Page::Connect | Page::Settings)
            || !self.page.available(self.source_engine())
        {
            self.page = Page::Overview;
        }
        self.refresh_now(cx);
        cx.notify();
    }

    fn source_engine(&self) -> SourceEngine {
        self.source
            .as_ref()
            .map(|s| s.engine())
            .unwrap_or(SourceEngine::ClickHouse)
    }

    fn hosts(&self) -> Vec<Host> {
        if std::env::var("CHM_SMOKE").is_ok() {
            return vec![Host {
                id: "smoke".into(),
                label: "smoke".into(),
                profile: ProfileConfig {
                    mode: Some("mock".into()),
                    ..Default::default()
                },
            }];
        }
        list_hosts(&load_config())
    }

    fn active_host_label(&self) -> String {
        let hosts = self.hosts();
        if let Some(id) = &self.active_host
            && let Some(h) = hosts.iter().find(|h| &h.id == id)
        {
            return h.label.clone();
        }
        self.source
            .as_ref()
            .map(|s| s.label())
            .unwrap_or_else(|| "no host".into())
    }

    fn host_status_text(&self) -> String {
        match self.conn {
            ConnState::Connecting => "connecting".into(),
            ConnState::Error => self.last_error.clone().unwrap_or_else(|| "error".into()),
            ConnState::Connected => {
                let mut parts = Vec::new();
                match self.host_status.health_ok {
                    Some(false) => parts.push("not ok".into()),
                    _ => parts.push("ok".into()),
                }
                if let Some(v) = &self.host_status.version
                    && !v.is_empty()
                {
                    parts.push(v.clone());
                }
                if self.host_status.replicas_total > 0 {
                    parts.push(format!(
                        "{}/{} replicas",
                        self.host_status.replicas_ok, self.host_status.replicas_total
                    ));
                }
                if let Some(ms) = self.host_status.fetch_ms {
                    parts.push(format!("{ms:.0}ms"));
                }
                parts.join(" · ")
            }
        }
    }

    /// Recurring refresh: each tick re-spawns itself, so a slow fetch can
    /// never overlap the next one. Exits once the shell is dropped.
    fn start_poll(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(POLL_SECS))
                    .await;
                let job = match this.update(cx, |shell, _| shell.poll_job()) {
                    Ok(job) => job,
                    Err(_) => return, // shell released — window closed
                };
                if let Some(job) = job {
                    apply_poll(job, &this, cx).await;
                }
            }
        })
        .detach();
    }

    /// Snapshot what a background fetch needs. Cheap: one Arc clone + a copy.
    fn poll_job(&self) -> Option<PollJob> {
        if matches!(self.page, Page::Connect | Page::Settings) {
            return None;
        }
        self.source.as_ref().map(|src| PollJob {
            src: src.clone(),
            page: self.page,
            range: self.range,
        })
    }

    /// Manual refresh action + initial fill.
    fn refresh_now(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.poll_job() else { return };
        if self.conn != ConnState::Error {
            self.conn = ConnState::Connecting;
        }
        cx.spawn(async move |this, cx| apply_poll(job, &this, cx).await)
            .detach();
    }

    fn apply_outcome(
        &mut self,
        outcome: PollOutcome,
        at: chrono::DateTime<chrono::Utc>,
        fetch_ms: f64,
        cx: &mut Context<Self>,
    ) {
        self.last_refresh = Some(at);
        self.host_status.fetch_ms = Some(fetch_ms);
        match outcome {
            PollOutcome::Overview { overview, traffic } => {
                if let Ok(o) = &overview {
                    self.host_status.version = Some(o.clickhouse_version.clone());
                    self.host_status.replicas_ok = o.replicas_ok;
                    self.host_status.replicas_total = o.replicas_total;
                }
                self.set_conn(overview.is_ok(), overview.as_ref().err().cloned());
                self.overview
                    .update(cx, |p, cx| p.set_overview(overview, traffic, cx));
            }
            PollOutcome::Queries {
                running,
                slow,
                failed,
            } => {
                let ok = running.is_ok() || slow.is_ok() || failed.is_ok();
                let err = running
                    .as_ref()
                    .err()
                    .or(slow.as_ref().err())
                    .or(failed.as_ref().err())
                    .cloned();
                self.set_conn(ok, err.filter(|_| !ok));
                self.queries
                    .update(cx, |p, cx| p.set(running, slow, failed, cx));
            }
            PollOutcome::Merges(data) => {
                self.set_conn(data.is_ok(), data.as_ref().err().cloned());
                self.merges.update(cx, |p, cx| p.set(data, cx));
            }
            PollOutcome::Replicas(data) => {
                self.set_conn(data.is_ok(), data.as_ref().err().cloned());
                self.replicas.update(cx, |p, cx| p.set(data, cx));
            }
            PollOutcome::Health(data) => {
                if let Ok(h) = &data {
                    self.host_status.health_ok = Some(h.ok);
                }
                self.set_conn(data.is_ok(), data.as_ref().err().cloned());
                self.health.update(cx, |p, cx| p.set(data, cx));
            }
            PollOutcome::Tables(data) => {
                self.set_conn(data.is_ok(), data.as_ref().err().cloned());
                self.tables.update(cx, |p, cx| p.set(data, cx));
            }
            PollOutcome::Traffic(data) => {
                self.set_conn(data.is_ok(), data.as_ref().err().cloned());
                self.traffic.update(cx, |p, cx| p.set(data, cx));
            }
        }
        cx.notify();
    }

    fn set_conn(&mut self, ok: bool, err: Option<String>) {
        if ok {
            self.conn = ConnState::Connected;
            self.last_error = None;
        } else {
            self.conn = ConnState::Error;
            self.last_error = err;
        }
    }

    // -- rendering ----------------------------------------------------------

    fn sidebar_toggle(
        &self,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        let glyph = if compact { "›" } else { "‹" };
        let label = if compact {
            div().child(SharedString::from(glyph))
        } else {
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.text_faint)
                        .child("Sidebar"),
                )
                .child(
                    div()
                        .text_color(theme.text_muted)
                        .child(SharedString::from(glyph)),
                )
        };
        div()
            .id("sidebar-toggle")
            .w_full()
            .px(px(if compact { 0.0 } else { 12.0 }))
            .py(px(6.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(|s| s.bg(theme.element_hover))
            .text_size(px(13.0))
            .when(compact, |el| el.flex().justify_center())
            .on_click(
                cx.listener(|this, _: &bezel::gpui::ClickEvent, window, cx| {
                    this.toggle_sidebar(window.viewport_size().width < px(COMPACT_BELOW), cx);
                }),
            )
            .child(label)
    }

    fn host_switcher(
        &self,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.active_host_label();
        let chevron = if self.host_menu_open { "▴" } else { "▾" };
        let trigger = div()
            .id("host-switcher")
            .w_full()
            .px(px(if compact { 0.0 } else { 12.0 }))
            .py(px(6.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(|s| s.bg(theme.element_hover))
            .when(compact, |el| el.flex().justify_center())
            .on_click(cx.listener(|this, _: &bezel::gpui::ClickEvent, _, cx| {
                this.host_menu_open = !this.host_menu_open;
                cx.notify();
            }))
            .child(if compact {
                div().child(status_dot(self.conn.dot(theme)))
            } else {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .w_full()
                    .child(status_dot(self.conn.dot(theme)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.0))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child(chevron),
                    )
            });

        let mut col = div().flex().flex_col().gap(px(2.0)).child(trigger);
        if self.host_menu_open {
            let active = self.active_host.clone();
            for host in self.hosts() {
                let id = host.id.clone();
                let selected = active.as_deref() == Some(id.as_str());
                let tag = match host.profile.mode.as_deref() {
                    Some("postgres") => " pg",
                    Some("clickhouse") => " ch",
                    Some("cloud") => " cloud",
                    _ => "",
                };
                let row_label = format!("{}{tag}", host.label);
                col = col.child(
                    div()
                        .id(SharedString::from(format!("host-{id}")))
                        .w_full()
                        .px(px(if compact { 0.0 } else { 12.0 }))
                        .py(px(5.0))
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .when(selected, |el| el.bg(theme.element_active))
                        .hover(|s| s.bg(theme.element_hover))
                        .text_size(px(12.0))
                        .when(compact, |el| el.flex().justify_center())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.switch_host(id.clone(), cx);
                        }))
                        .child(if compact {
                            div()
                                .text_size(px(10.0))
                                .child(row_label.chars().next().unwrap_or('·').to_string())
                        } else {
                            div().truncate().child(row_label)
                        }),
                );
            }
            col = col.child(
                div()
                    .id("host-add")
                    .w_full()
                    .px(px(if compact { 0.0 } else { 12.0 }))
                    .py(px(5.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .when(compact, |el| el.flex().justify_center())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.host_menu_open = false;
                        this.page = Page::Connect;
                        cx.notify();
                    }))
                    .child(if compact {
                        SharedString::from("+")
                    } else {
                        SharedString::from("+ Add host")
                    }),
            );
        }
        col
    }

    fn sidebar(&self, theme: &Theme, compact: bool, cx: &mut Context<Self>) -> bezel::gpui::Div {
        let engine = self.source_engine();
        let items: Vec<bezel::gpui::AnyElement> = Page::ALL
            .iter()
            .copied()
            .filter(|page| page.available(engine))
            .enumerate()
            .map(|(i, page)| {
                let active = page == self.page;
                let hotkey = format!("{}", i + 1);
                let label = if compact {
                    div().child(page.icon())
                } else {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(page.icon())
                        .child(div().child(page.title()))
                        .child(
                            div()
                                .ml(px(2.0))
                                .text_size(px(10.0))
                                .text_color(theme.text_faint)
                                .child(hotkey),
                        )
                };
                div()
                    .id(SharedString::from(format!("nav-{}", page.title())))
                    .w_full()
                    .px(px(if compact { 0.0 } else { 12.0 }))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .when(active, |el| el.bg(theme.element_active))
                    .hover(|s| s.bg(theme.element_hover))
                    .text_size(px(13.0))
                    .text_color(if active { theme.text } else { theme.text_muted })
                    .on_click(
                        cx.listener(move |this, _: &bezel::gpui::ClickEvent, _, cx| {
                            this.goto(page, cx);
                        }),
                    )
                    .child(label)
            })
            .map(bezel::gpui::IntoElement::into_any_element)
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .when(compact, |col| col.items_center())
            .children(items)
    }

    fn settings_nav(
        &self,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.page == Page::Settings;
        let label = if compact {
            div().child(Page::Settings.icon())
        } else {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(Page::Settings.icon())
                .child(div().child(Page::Settings.title()))
                .child(
                    div()
                        .ml(px(2.0))
                        .text_size(px(10.0))
                        .text_color(theme.text_faint)
                        .child("⌘,"),
                )
        };
        div()
            .id("nav-Settings")
            .w_full()
            .px(px(if compact { 0.0 } else { 12.0 }))
            .py(px(6.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .when(active, |el| el.bg(theme.element_active))
            .hover(|s| s.bg(theme.element_hover))
            .text_size(px(13.0))
            .text_color(if active { theme.text } else { theme.text_muted })
            .when(compact, |el| el.flex().justify_center())
            .on_click(cx.listener(|this, _: &bezel::gpui::ClickEvent, _, cx| {
                this.page = Page::Settings;
                cx.notify();
            }))
            .child(label)
    }

    fn range_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> bezel::gpui::Div {
        let mut row = div().flex().flex_row().items_center().gap(px(4.0));
        for range in TimeRange::ALL {
            let active = self.range == range;
            row = row.child(
                div()
                    .id(SharedString::from(format!("range-{}", range.label())))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(11.5))
                    .when(active, |el| {
                        el.bg(theme.element_active).text_color(theme.text)
                    })
                    .when(!active, |el| el.text_color(theme.text_muted))
                    .hover(|s| s.bg(theme.element_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.range != range {
                            this.range = range;
                            this.refresh_now(cx);
                            cx.notify();
                        }
                    }))
                    .child(range.label()),
            );
        }
        row
    }

    fn status_bar(&self, theme: &Theme) -> bezel::gpui::Div {
        let host = SharedString::from(self.active_host_label());
        let status = SharedString::from(self.host_status_text());
        let status_color = match self.conn {
            ConnState::Error => theme.danger,
            ConnState::Connecting => theme.warning,
            ConnState::Connected => theme.text_muted,
        };
        let refreshed = match self.last_refresh {
            Some(at) => format!("updated {}", at.format("%H:%M:%S")),
            None => "not refreshed yet".to_string(),
        };
        let note = match &self.update_note {
            None => Some(SharedString::from("update check…")),
            Some(UpdateNote(t)) if !t.is_empty() => Some(t.clone()),
            Some(_) => None,
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(6.0))
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_size(px(11.5))
            .text_color(theme.text_muted)
            .child(status_dot(self.conn.dot(theme)))
            .child(div().min_w_0().truncate().child(host))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(status_color)
                    .child(status),
            )
            .child(div().child(refreshed))
            .children(note.map(|t| div().text_color(theme.text_faint).child(t)))
    }

    fn content(&mut self, _cx: &mut Context<Self>) -> bezel::gpui::AnyElement {
        // No source yet: Connect owns the pane whatever the route points at.
        if self.source.is_none() && !matches!(self.page, Page::Connect | Page::Settings) {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(bezel::theme::ink(0.55))
                .text_size(px(13.0))
                .child("no connection configured — pick a mode in Connect")
                .into_any_element();
        }
        match self.page {
            Page::Overview => self.overview.clone().into_any_element(),
            Page::Queries => self.queries.clone().into_any_element(),
            Page::Merges => self.merges.clone().into_any_element(),
            Page::Replicas => self.replicas.clone().into_any_element(),
            Page::Health => self.health.clone().into_any_element(),
            Page::Tables => self.tables.clone().into_any_element(),
            Page::Traffic => self.traffic.clone().into_any_element(),
            Page::Connect => self.connect.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Cloned so helpers can take &mut cx while holding theme tokens
        // (Theme::of borrows cx for the whole expression otherwise).
        let theme = Theme::of(cx).clone();
        // Hover fades paint once and stick unless frames are requested.
        if bezel::motion::hover_fades_active() {
            window.request_animation_frame();
        }

        let viewport = window.viewport_size();
        let narrow = viewport.width < px(COMPACT_BELOW);
        let compact = sidebar_is_compact(self.sidebar_collapsed, narrow);
        let show_range = self.page.uses_range() && self.source.is_some();

        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh_now(cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                this.toggle_sidebar(window.viewport_size().width < px(COMPACT_BELOW), cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.page = Page::Settings;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Only when the shell itself holds focus — digits typed into
                // a Connect text field must not switch pages.
                if !this.focus.is_focused(window) {
                    return;
                }
                let m = &event.keystroke.modifiers;
                if m.control || m.alt || m.shift {
                    return;
                }
                if let Some(idx) = event
                    .keystroke
                    .key
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                {
                    let engine = this.source_engine();
                    let page = Page::ALL
                        .iter()
                        .copied()
                        .filter(|p| p.available(engine))
                        .nth(idx);
                    if let Some(page) = page {
                        this.goto(page, cx);
                    }
                }
            }))
            .flex()
            .flex_row()
            .flex_1()
            .bg(theme.bg)
            .text_color(theme.text)
            .size_full()
            .child(
                // Sidebar surface; collapses to an icon strip below 900px.
                div()
                    .flex()
                    .flex_col()
                    .w(px(if compact {
                        SIDEBAR_W_COMPACT
                    } else {
                        SIDEBAR_W
                    }))
                    .h_full()
                    .flex_none()
                    .p(px(8.0))
                    .gap(px(8.0))
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .child(self.sidebar_toggle(&theme, compact, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .when(compact, |col| col.items_center())
                            .child(self.host_switcher(&theme, compact, cx))
                            .child(self.sidebar(&theme, compact, cx))
                            .child(div().flex_1())
                            .child(self.settings_nav(&theme, compact, cx))
                            .pb(px(28.0)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .px(px(16.0))
                            .pt(px(12.0))
                            .pb(px(4.0))
                            .gap(px(12.0))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .child(SharedString::from(self.page.title())),
                            )
                            .child(div().flex_1())
                            .when(show_range, |row| row.child(self.range_bar(&theme, cx))),
                    )
                    .child(
                        div()
                            .id("content-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .p(px(16.0))
                            .overflow_y_scroll()
                            .child(self.content(cx)),
                    )
                    .child(self.status_bar(&theme)),
            )
    }
}

// ---------------------------------------------------------------------------
// Background polling plumbing
// ---------------------------------------------------------------------------

/// Everything a background fetch needs, cloned out of the view so no borrow
/// crosses an await point.
#[derive(Clone)]
struct PollJob {
    src: Arc<Box<dyn DataSource>>,
    page: Page,
    range: TimeRange,
}

enum PollOutcome {
    Overview {
        overview: Result<Overview, String>,
        traffic: Result<TrafficSeries, String>,
    },
    Queries {
        running: Result<Vec<QueryRow>, String>,
        slow: Result<Vec<QueryRow>, String>,
        failed: Result<Vec<QueryRow>, String>,
    },
    Merges(Result<Vec<MergeRow>, String>),
    Replicas(Result<Vec<ReplicaRow>, String>),
    Health(Result<Health, String>),
    Tables(Result<Vec<TableStat>, String>),
    Traffic(Result<TrafficSeries, String>),
}

fn map_err<T>(r: chm_core::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

async fn apply_poll(job: PollJob, this: &WeakEntity<Shell>, cx: &mut AsyncApp) {
    let started = Instant::now();
    let src = job.src;
    let range = job.range;
    let outcome = match job.page {
        Page::Connect | Page::Settings => return,
        Page::Overview => {
            let (overview, traffic) = chm_core::tokio_block_on(async {
                tokio::join!(src.overview(range), src.traffic(range))
            });
            PollOutcome::Overview {
                overview: map_err(overview),
                traffic: map_err(traffic),
            }
        }
        Page::Queries => {
            let (running, slow, failed) = chm_core::tokio_block_on(async {
                tokio::join!(
                    src.running_queries(),
                    src.slow_queries(range),
                    src.failed_queries(range)
                )
            });
            PollOutcome::Queries {
                running: map_err(running),
                slow: map_err(slow),
                failed: map_err(failed),
            }
        }
        Page::Merges => PollOutcome::Merges(map_err(chm_core::tokio_block_on(src.merges()))),
        Page::Replicas => PollOutcome::Replicas(map_err(chm_core::tokio_block_on(src.replicas()))),
        Page::Health => PollOutcome::Health(map_err(chm_core::tokio_block_on(src.health()))),
        Page::Tables => PollOutcome::Tables(map_err(chm_core::tokio_block_on(src.tables()))),
        Page::Traffic => {
            PollOutcome::Traffic(map_err(chm_core::tokio_block_on(src.traffic(range))))
        }
    };
    // Telemetry hook: fetch latency lands in PerfMetrics whenever the process
    // global exists; recording itself is opt-in via config.toml at startup.
    let fetch_ms = started.elapsed().as_secs_f64() * 1000.0;
    let _ = perf().record_fetch(fetch_ms);
    let at = chrono::Utc::now();
    let _ = this.update(cx, |shell, cx| {
        shell.apply_outcome(outcome, at, fetch_ms, cx)
    });
}

/// Compact (icon strip) when the user collapsed it, otherwise when the
/// window is narrower than [`COMPACT_BELOW`].
fn sidebar_is_compact(user: Option<bool>, narrow: bool) -> bool {
    user.unwrap_or(narrow)
}

#[cfg(test)]
mod tests {
    use super::sidebar_is_compact;

    #[test]
    fn sidebar_follows_viewport_until_toggled() {
        assert!(!sidebar_is_compact(None, false));
        assert!(sidebar_is_compact(None, true));
        assert!(sidebar_is_compact(Some(true), false));
        assert!(!sidebar_is_compact(Some(false), true));
        assert!(sidebar_is_compact(Some(true), true));
        assert!(!sidebar_is_compact(Some(false), false));
    }
}
