//! App shell — window layout, sidebar nav, page routing, status bar,
//! 30-second poll loop and the startup update-check hook.
//! AGENT D OWNS THIS FILE.
//!
//! All gpui types come through `bezel::gpui`; widgets come from the bezel
//! facade (`bezel::theme`, `bezel::ui`).

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use chm_clickhouse::ClickHouseClient;
use chm_cloud_api::CloudClient;
use chm_core::{DataSource, MockDataSource};

use bezel::gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, FocusHandle, Focusable, Hsla, KeyBinding,
    KeyDownEvent, Render, SharedString, WeakEntity, Window, actions, div, prelude::*, px,
};
use bezel::theme::Theme;
use bezel::ui::widgets::status_dot;

use crate::connect::{ConnectEvent, ConnectFlow};
use crate::pages::Page;
use crate::pages::overview::OverviewPage;

actions!(chm_shell, [Refresh]);

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
// config.toml schema
// ---------------------------------------------------------------------------

/// Saved connection profile (`[profile]` table). Written by the Connect
/// screen's Save button, read at startup.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
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
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct TelemetrySection {
    #[serde(default)]
    pub enabled: bool,
}

/// Whole `config.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub profile: ProfileConfig,
    #[serde(default)]
    pub telemetry: TelemetrySection,
}

/// `<config_dir>/chmonitor/config.toml`.
pub fn config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("chmonitor").join("config.toml"))
}

/// Read the saved profile, if a well-formed file exists. Any failure means
/// "no profile" and the app shows the Connect screen.
pub fn load_profile() -> Option<ProfileConfig> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    let cfg: ConfigFile = toml::from_str(&text).ok()?;
    cfg.profile.mode.as_ref()?;
    Some(cfg.profile)
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

/// The root view: owns routing, the active data source and the poll task.
pub struct Shell {
    focus: FocusHandle,
    page: Page,
    source: Option<Arc<Box<dyn DataSource>>>,
    conn: ConnState,
    last_refresh: Option<chrono::DateTime<chrono::Utc>>,
    overview: Option<Entity<OverviewPage>>,
    connect: Entity<ConnectFlow>,
    update_note: Option<UpdateNote>,
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
    fn pick_source() -> (Option<Arc<Box<dyn DataSource>>>, ConnState) {
        if std::env::var("CHM_SMOKE").is_ok() {
            return (
                Some(Arc::new(
                    Box::new(MockDataSource::new("mock (CHM_SMOKE)")) as Box<dyn DataSource>
                )),
                ConnState::Connected,
            );
        }
        match load_profile() {
            Some(profile) => match source_from_profile(&profile) {
                Some(src) => (Some(Arc::new(src)), ConnState::Connecting),
                None => (None, ConnState::Error),
            },
            None => (None, ConnState::Error),
        }
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let (source, conn) = Self::pick_source();

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
            let current = semver::Version::new(0, 1, 0);
            let note = match chm_core::tokio_block_on(checker.check(channel, &current)) {
                Ok(Some(release)) => {
                    UpdateNote(format!("update available: v{}", release.version()).into())
                }
                Ok(None) => UpdateNote("up to date".into()),
                Err(_) => return,
            };
            let _ = this.update(cx, |shell, cx| {
                shell.update_note = Some(note);
                cx.notify();
            });
        })
        .detach();

        let mut shell = Self {
            focus: cx.focus_handle(),
            page: Page::Overview,
            source,
            conn,
            last_refresh: None,
            overview: None,
            connect: cx.new(|cx| ConnectFlow::new(load_profile(), cx)),
            update_note: None,
        };

        // Digits 1-8 switch pages; handled in render's on_key_down so it works
        // wherever focus sits in this view's subtree. `r` is an action.
        cx.bind_keys([KeyBinding::new("r", Refresh, None)]);

        // Rebuild the source after the Connect screen writes a new profile.
        cx.subscribe(&shell.connect, |this, _, event: &ConnectEvent, cx| {
            let ConnectEvent::SavedProfile(profile) = event;
            this.source = source_from_profile(profile).map(Arc::new);
            this.conn = if this.source.is_some() {
                ConnState::Connecting
            } else {
                ConnState::Error
            };
            this.refresh_now(cx);
            cx.notify();
        })
        .detach();

        shell.start_poll(cx);
        shell.refresh_now(cx);
        shell
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
        self.source.as_ref().map(|src| PollJob {
            src: src.clone(),
            page: self.page,
        })
    }

    /// Manual refresh action + initial fill.
    fn refresh_now(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.poll_job() else { return };
        cx.spawn(async move |this, cx| apply_poll(job, &this, cx).await)
            .detach();
    }

    /// Land fetched data back on the view (called from the async context).
    fn set_overview_data(
        &mut self,
        data: Result<chm_core::Overview, String>,
        at: chrono::DateTime<chrono::Utc>,
        cx: &mut Context<Self>,
    ) {
        self.last_refresh = Some(at);
        self.conn = if data.is_ok() {
            ConnState::Connected
        } else {
            ConnState::Error
        };
        if self.overview.is_none() {
            self.overview = Some(cx.new(|_| OverviewPage::new()));
        }
        if let Some(page) = &self.overview {
            page.update(cx, |p, cx| p.set_overview(data, cx));
        }
        cx.notify();
    }

    // -- rendering ----------------------------------------------------------

    fn sidebar(&self, theme: &Theme, compact: bool, cx: &mut Context<Self>) -> bezel::gpui::Div {
        let items: Vec<bezel::gpui::AnyElement> = Page::ALL
            .iter()
            .map(|&page| {
                let active = page == self.page;
                let hotkey = format!("{}", page.index() + 1);
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
                            this.page = page;
                            cx.notify();
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

    fn status_bar(&self, theme: &Theme) -> bezel::gpui::Div {
        let label = self
            .source
            .as_ref()
            .map(|s| SharedString::from(s.label()))
            .unwrap_or_else(|| "no source configured".into());
        let refreshed = match self.last_refresh {
            Some(at) => format!("updated {}", at.format("%H:%M:%S")),
            None => "not refreshed yet".to_string(),
        };
        let note = self
            .update_note
            .as_ref()
            .map(|UpdateNote(t)| t.clone())
            .unwrap_or_else(|| "update check…".into());

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
            .child(div().min_w_0().truncate().child(label))
            .child(div().child(refreshed))
            .child(div().flex_1())
            .child(div().text_color(theme.text_faint).child(note))
    }

    fn content(&mut self, _cx: &mut Context<Self>) -> bezel::gpui::AnyElement {
        // No source yet: Connect owns the pane whatever the route points at.
        if self.source.is_none() && self.page != Page::Connect {
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
            Page::Overview => match &self.overview {
                Some(page) => page.clone().into_any_element(),
                None => placeholder("loading overview…").into_any_element(),
            },
            // Placeholder routes until Agents F/G/H replace pages/.
            Page::Queries => placeholder("Queries — owned by Agent F").into_any_element(),
            Page::Merges => placeholder("Merges — owned by Agent F").into_any_element(),
            Page::Replicas => placeholder("Replicas — owned by Agent G").into_any_element(),
            Page::Health => placeholder("Health — owned by Agent G").into_any_element(),
            Page::Tables => placeholder("Tables — owned by Agent H").into_any_element(),
            Page::Traffic => placeholder("Traffic — owned by Agent H").into_any_element(),
            Page::Connect => self.connect.clone().into_any_element(),
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
        let compact = viewport.width < px(COMPACT_BELOW);

        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh_now(cx)))
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
                    && let Some(&page) = Page::ALL.get(idx)
                {
                    this.page = page;
                    cx.notify();
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
                    .child(self.sidebar(&theme, compact, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
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
}

async fn apply_poll(job: PollJob, this: &WeakEntity<Shell>, cx: &mut AsyncApp) {
    let started = Instant::now();
    let result = match job.page {
        Page::Overview => fetch_overview(&job.src).await,
        _ => return,
    };
    // Telemetry hook: fetch latency lands in PerfMetrics whenever the process
    // global exists; recording itself is opt-in via config.toml at startup.
    let _ = perf().record_fetch(started.elapsed().as_secs_f64() * 1000.0);
    let at = chrono::Utc::now();
    let _ = this.update(cx, |shell, cx| shell.set_overview_data(result, at, cx));
}

async fn fetch_overview(src: &Arc<Box<dyn DataSource>>) -> Result<chm_core::Overview, String> {
    chm_core::tokio_block_on(src.overview(chm_core::TimeRange::TwentyFourHours))
        .map_err(|e| e.to_string())
}

fn placeholder(text: &'static str) -> bezel::gpui::Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_color(bezel::theme::ink(0.45))
        .text_size(px(13.0))
        .child(text)
}
