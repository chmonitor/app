//! App shell — window layout, sidebar nav, page routing, status bar,
//! 30-second poll loop and the startup update-check hook.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chm_core::{
    DataSource, Health, MergeRow, MockDataSource, Overview, QueryRow, ReplicaRow, SourceEngine,
    TableStat, TimeRange, TrafficSeries,
};

use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, FocusHandle, Focusable, Hsla, KeyBinding,
    KeyDownEvent, Render, SharedString, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Root, h_flex,
    sidebar::{
        Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
        SidebarToggleButton,
    },
    status_bar::StatusBar,
    v_flex,
};

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
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Connected => cx.theme().green,
            Self::Connecting => cx.theme().warning,
            Self::Error => cx.theme().danger,
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

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            connect: cx.new(|cx| ConnectFlow::new(load_profile(), window, cx)),
            settings: cx.new(|_| SettingsPage::new()),
            update_note: None,
            sidebar_collapsed: None,
            active_host,
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
        self.last_error = None;
    }

    fn switch_host(&mut self, host_id: String, cx: &mut Context<Self>) {
        if self.active_host.as_deref() == Some(host_id.as_str()) {
            cx.notify();
            return;
        }
        if std::env::var("CHM_SMOKE").is_ok() {
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

    fn host_icon(mode: Option<&str>) -> IconName {
        match mode {
            Some("postgres") => IconName::HardDrive,
            Some("cloud") => IconName::Globe,
            _ => IconName::Cpu,
        }
    }

    // -- rendering ----------------------------------------------------------

    fn render_sidebar(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let engine = self.source_engine();
        let active_host = self.active_host.clone();
        let hosts = self.hosts();

        let mut host_menu = SidebarMenu::new();
        for host in hosts {
            let id = host.id.clone();
            let selected = active_host.as_deref() == Some(id.as_str());
            let icon = Self::host_icon(host.profile.mode.as_deref());
            host_menu = host_menu.child(
                SidebarMenuItem::new(host.label.clone())
                    .icon(icon)
                    .active(selected)
                    .on_click(cx.listener(move |this, _, _, cx| this.switch_host(id.clone(), cx))),
            );
        }
        host_menu = host_menu.child(
            SidebarMenuItem::new("Add host")
                .icon(IconName::Plus)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Connect;
                    cx.notify();
                })),
        );

        let mut nav = SidebarMenu::new();
        for (i, page) in Page::ALL
            .iter()
            .copied()
            .filter(|page| page.available(engine))
            .enumerate()
        {
            let active = page == self.page;
            let hotkey = format!("{}", i + 1);
            nav = nav.child(
                SidebarMenuItem::new(page.title())
                    .icon(page.icon())
                    .active(active)
                    .suffix({
                        let hotkey = hotkey.clone();
                        let muted = cx.theme().muted_foreground;
                        move |_, _| div().text_xs().text_color(muted).child(hotkey.clone())
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.goto(page, cx))),
            );
        }

        Sidebar::new("nav")
            .collapsed(compact)
            .collapsible(true)
            .w(px(SIDEBAR_W))
            .header(
                SidebarHeader::new().child(
                    h_flex().w_full().items_center().justify_between().child(
                        SidebarToggleButton::new()
                            .collapsed(compact)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_sidebar(
                                    window.viewport_size().width < px(COMPACT_BELOW),
                                    cx,
                                );
                            })),
                    ),
                ),
            )
            .child(SidebarGroup::new("Host").child(host_menu))
            .child(SidebarGroup::new("Monitor").child(nav))
            .child(
                SidebarGroup::new("App").child(
                    SidebarMenu::new().child(
                        SidebarMenuItem::new(Page::Settings.title())
                            .icon(Page::Settings.icon())
                            .active(self.page == Page::Settings)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.page = Page::Settings;
                                cx.notify();
                            })),
                    ),
                ),
            )
            .footer(SidebarFooter::new().child("chmonitor"))
    }

    fn range_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let mut group = crate::widgets::controls::range_group("time-range");
        for range in TimeRange::ALL {
            let entity = entity.clone();
            let pressed = self.range == range;
            group = group.child(
                crate::widgets::controls::range_toggle(
                    format!("range-{}", range.label()),
                    pressed,
                    range.label(),
                    cx,
                )
                .on_change(move |next, _, _, cx| {
                    if next {
                        let _ = entity.update(cx, |this, cx| {
                            if this.range != range {
                                this.range = range;
                                this.refresh_now(cx);
                                cx.notify();
                            }
                        });
                    }
                }),
            );
        }
        group
    }

    fn status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let host = self.active_host_label();
        let status = self.host_status_text();
        let refreshed = match self.last_refresh {
            Some(at) => format!("updated {}", at.format("%H:%M:%S")),
            None => "not refreshed yet".to_string(),
        };
        let note = match &self.update_note {
            None => Some(SharedString::from("update check…")),
            Some(UpdateNote(t)) if !t.is_empty() => Some(t.clone()),
            Some(_) => None,
        };
        StatusBar::new()
            .left(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(self.conn.color(cx)))
                    .child(host),
            )
            .child(div().text_color(self.conn.color(cx)).child(status))
            .right(refreshed)
            .children(note.map(|t| div().text_color(cx.theme().muted_foreground).child(t)))
    }

    fn content(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.source.is_none() && !matches!(self.page, Page::Connect | Page::Settings) {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .text_sm()
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
        let viewport = window.viewport_size();
        let narrow = viewport.width < px(COMPACT_BELOW);
        let compact = sidebar_is_compact(self.sidebar_collapsed, narrow);
        let show_range = self.page.uses_range() && self.source.is_some();

        h_flex()
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
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(compact, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            .items_center()
                            .px_4()
                            .pt_3()
                            .pb_1()
                            .gap_3()
                            .child(div().text_lg().child(SharedString::from(self.page.title())))
                            .child(div().flex_1())
                            .when(show_range, |row| row.child(self.range_bar(cx))),
                    )
                    .child(
                        div()
                            .id("content-scroll")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .p_4()
                            .overflow_y_scroll()
                            .child(self.content(cx)),
                    )
                    .child(self.status_bar(cx)),
            )
            .children(Root::render_notification_layer(window, cx))
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
