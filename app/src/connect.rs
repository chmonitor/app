//! Connect screen — Cloud API vs ClickHouse vs Postgres, Test (ping) and Save.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString,
    Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{Input, InputState},
    v_flex,
};

use crate::widgets::controls::{choice_radio, ghost_button, primary_button, radio_group};

use crate::config::{
    DEFAULT_HOST_ID, ProfileConfig, host_id_from_name, load_config, save_config,
    source_from_profile,
};

/// Fired after Save successfully writes config.toml.
#[derive(Debug, Clone)]
pub enum ConnectEvent {
    SavedProfile {
        profile: ProfileConfig,
        host_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Cloud,
    ClickHouse,
    Postgres,
}

/// Outcome of the last Test press.
#[derive(Debug, Clone)]
enum TestState {
    Idle,
    Testing,
    Ok,
    Failed(String),
}

pub struct ConnectFlow {
    focus: FocusHandle,
    mode: Mode,
    base_url: Entity<InputState>,
    api_key: Entity<InputState>,
    url: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    database: Entity<InputState>,
    name: Entity<InputState>,
    test: TestState,
}

impl EventEmitter<ConnectEvent> for ConnectFlow {}

impl Focusable for ConnectFlow {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConnectFlow {
    /// `initial` prefills the form from a saved profile, if any.
    pub fn new(
        initial: Option<ProfileConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = |text: Option<String>,
                     placeholder: &'static str,
                     masked: bool,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut f = InputState::new(window, cx).placeholder(placeholder);
                if let Some(t) = text.filter(|t| !t.is_empty()) {
                    f = f.default_value(t);
                }
                if masked {
                    f = f.masked(true);
                }
                f
            })
        };
        let initial = initial.unwrap_or_default();
        let mode = match initial.mode.as_deref() {
            Some("clickhouse") => Mode::ClickHouse,
            Some("postgres") => Mode::Postgres,
            _ => Mode::Cloud,
        };
        let default_user = match mode {
            Mode::Postgres => "postgres",
            _ => "default",
        };
        Self {
            focus: cx.focus_handle(),
            mode,
            base_url: field(
                initial.base_url,
                "https://acme.dash.chmonitor.dev",
                false,
                window,
                cx,
            ),
            api_key: field(initial.api_key, "API key", true, window, cx),
            url: field(
                initial.url,
                if matches!(mode, Mode::Postgres) {
                    "postgres://localhost:5432/postgres"
                } else {
                    "http://localhost:8123"
                },
                false,
                window,
                cx,
            ),
            user: field(
                initial.user.or(Some(default_user.into())),
                "user",
                false,
                window,
                cx,
            ),
            password: field(initial.password, "password", true, window, cx),
            database: field(
                initial.database.or(Some("postgres".into())),
                "database",
                false,
                window,
                cx,
            ),
            name: field(None, "work (optional)", false, window, cx),
            test: TestState::Idle,
        }
    }

    fn read(e: &Entity<InputState>, cx: &Context<Self>) -> String {
        e.read(cx).value().trim().to_string()
    }

    /// Collect the form into a profile. Returns None when fields required by
    /// the selected mode are missing.
    fn profile_from_form(&self, cx: &Context<Self>) -> Option<ProfileConfig> {
        fn nonempty(s: String) -> Option<String> {
            (!s.is_empty()).then_some(s)
        }
        match self.mode {
            Mode::Cloud => {
                let base_url = nonempty(Self::read(&self.base_url, cx))?;
                Some(ProfileConfig {
                    mode: Some("cloud".into()),
                    base_url: Some(base_url),
                    api_key: nonempty(Self::read(&self.api_key, cx)),
                    ..ProfileConfig::default()
                })
            }
            Mode::ClickHouse => {
                let url = nonempty(Self::read(&self.url, cx))?;
                Some(ProfileConfig {
                    mode: Some("clickhouse".into()),
                    url: Some(url),
                    user: nonempty(Self::read(&self.user, cx)),
                    password: nonempty(Self::read(&self.password, cx)),
                    ..ProfileConfig::default()
                })
            }
            Mode::Postgres => {
                let url = nonempty(Self::read(&self.url, cx))?;
                Some(ProfileConfig {
                    mode: Some("postgres".into()),
                    url: Some(url),
                    user: nonempty(Self::read(&self.user, cx)),
                    password: nonempty(Self::read(&self.password, cx)),
                    database: nonempty(Self::read(&self.database, cx)),
                    ..ProfileConfig::default()
                })
            }
        }
    }

    fn run_test(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.profile_from_form(cx) else {
            self.test = TestState::Failed("fill the required fields first".into());
            cx.notify();
            return;
        };
        let Some(src) = source_from_profile(&profile) else {
            self.test = TestState::Failed("could not build client from these settings".into());
            cx.notify();
            return;
        };
        self.test = TestState::Testing;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = chm_core::tokio_block_on(src.ping());
            let _ = this.update(cx, |flow, cx| {
                flow.test = match result {
                    Ok(()) => TestState::Ok,
                    Err(e) => TestState::Failed(e.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.profile_from_form(cx) else {
            self.test = TestState::Failed("fill the required fields first".into());
            cx.notify();
            return;
        };
        let host_id = host_id_from_name(&Self::read(&self.name, cx));
        let mut cfg = load_config();
        if host_id == DEFAULT_HOST_ID {
            cfg.profile = profile.clone();
        } else {
            cfg.profiles.insert(host_id.clone(), profile.clone());
        }
        cfg.ui.host = Some(host_id.clone());
        match save_config(&cfg) {
            Ok(()) => {
                self.test = TestState::Ok;
                cx.emit(ConnectEvent::SavedProfile { profile, host_id });
                cx.notify();
            }
            Err(e) => {
                self.test = TestState::Failed(e);
                cx.notify();
            }
        }
    }

    fn mode_row(
        &self,
        label: &'static str,
        hint: &'static str,
        mode: Mode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.mode == mode;
        let entity = cx.entity().downgrade();
        choice_radio(format!("mode-{label}"), selected, label, hint, cx).on_change(
            move |next, _, _, cx| {
                if next {
                    let _ = entity.update(cx, |this, cx| {
                        this.mode = mode;
                        this.test = TestState::Idle;
                        cx.notify();
                    });
                }
            },
        )
    }

    fn field_row(label: &'static str, field: &Entity<InputState>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(div().text_sm().child(label))
            .child(Input::new(field))
    }

    fn status_line(&self, cx: &Context<Self>) -> impl IntoElement {
        let (text, color) = match &self.test {
            TestState::Idle => ("", cx.theme().muted_foreground),
            TestState::Testing => ("testing…", cx.theme().warning),
            TestState::Ok => ("connection ok", cx.theme().green),
            TestState::Failed(e) => (e.as_str(), cx.theme().danger),
        };
        div()
            .min_h(px(18.))
            .text_sm()
            .text_color(color)
            .child(SharedString::from(text.to_string()))
    }
}

impl Render for ConnectFlow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = match self.mode {
            Mode::Cloud => v_flex()
                .gap_3()
                .child(Self::field_row("Base URL", &self.base_url))
                .child(Self::field_row("API key", &self.api_key)),
            Mode::ClickHouse => v_flex()
                .gap_3()
                .child(Self::field_row("URL", &self.url))
                .child(Self::field_row("User", &self.user))
                .child(Self::field_row("Password", &self.password)),
            Mode::Postgres => v_flex()
                .gap_3()
                .child(Self::field_row("URL", &self.url))
                .child(Self::field_row("User", &self.user))
                .child(Self::field_row("Password", &self.password))
                .child(Self::field_row("Database", &self.database)),
        };

        v_flex()
            .gap_4()
            .max_w(px(460.))
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_lg().child("Add a host"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("ClickHouse, Postgres, or the chmonitor cloud API."),
                    ),
            )
            .child(
                radio_group("connect-mode")
                    .child(self.mode_row(
                        "Cloud",
                        "chmonitor-hosted dashboard API · base URL + API key",
                        Mode::Cloud,
                        cx,
                    ))
                    .child(self.mode_row(
                        "ClickHouse",
                        "HTTP endpoint · url + user + password",
                        Mode::ClickHouse,
                        cx,
                    ))
                    .child(self.mode_row(
                        "Postgres",
                        "libpq endpoint · url + user + password + database",
                        Mode::Postgres,
                        cx,
                    )),
            )
            .child(fields)
            .child(Self::field_row("Name", &self.name))
            .child(self.status_line(cx))
            .child({
                let entity = cx.entity().downgrade();
                h_flex()
                    .gap_2()
                    .child(ghost_button("test", "Test", cx).on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| this.run_test(cx));
                        }
                    }))
                    .child(
                        primary_button("save", "Save", cx).on_click(move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| this.save(cx));
                        }),
                    )
            })
    }
}
