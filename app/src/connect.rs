//! Connect screen — two-mode connection form (Cloud API vs direct
//! ClickHouse), Test (ping) and Save (writes config.toml).
//! AGENT D OWNS THIS FILE.
//!
//! Flow: pick mode → fill the relevant fields → Test runs `DataSource::ping`
//! → Save persists `[profile]` to `<config_dir>/chmonitor/config.toml` and
//! emits [`ConnectEvent::SavedProfile`], which shell.rs turns into a live
//! data source.

use bezel::gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString,
    div, prelude::*, px,
};
use bezel::theme::Theme;
use bezel::ui::input::TextField;
use bezel::ui::widgets::{ButtonStyle, Buttons};

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
    base_url: Entity<TextField>,
    api_key: Entity<TextField>,
    url: Entity<TextField>,
    user: Entity<TextField>,
    password: Entity<TextField>,
    database: Entity<TextField>,
    name: Entity<TextField>,
    test: TestState,
}

impl EventEmitter<ConnectEvent> for ConnectFlow {}

impl Focusable for ConnectFlow {
    fn focus_handle(&self, _: &bezel::gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConnectFlow {
    /// `initial` prefills the form from a saved profile, if any.
    pub fn new(initial: Option<ProfileConfig>, cx: &mut Context<Self>) -> Self {
        let field = |text: Option<String>, placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut f = TextField::new(cx).with_placeholder(placeholder);
                if let Some(t) = text.filter(|t| !t.is_empty()) {
                    f.set_content(t, cx);
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
            base_url: field(initial.base_url, "https://acme.dash.chmonitor.dev", cx),
            api_key: field(initial.api_key, "API key", cx),
            url: field(
                initial.url,
                if matches!(mode, Mode::Postgres) {
                    "postgres://localhost:5432/postgres"
                } else {
                    "http://localhost:8123"
                },
                cx,
            ),
            user: field(initial.user.or(Some(default_user.into())), "user", cx),
            password: field(initial.password, "password", cx),
            database: field(initial.database.or(Some("postgres".into())), "database", cx),
            name: field(None, "work (optional)", cx),
            test: TestState::Idle,
        }
    }

    fn read(e: &Entity<TextField>, cx: &Context<Self>) -> String {
        e.read(cx).content().trim().to_string()
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

    // -- rendering ----------------------------------------------------------

    fn mode_row(
        &self,
        theme: &Theme,
        label: &'static str,
        hint: &'static str,
        mode: Mode,
        cx: &mut Context<Self>,
    ) -> bezel::gpui::Stateful<bezel::gpui::Div> {
        let selected = self.mode == mode;
        div()
            .id(SharedString::from(format!("mode-{label}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(if selected {
                theme.border_strong
            } else {
                theme.border
            })
            .bg(if selected {
                theme.element_active
            } else {
                theme.input_bg
            })
            .cursor_pointer()
            .hover(|s| s.bg(theme.element_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.mode = mode;
                this.test = TestState::Idle;
                cx.notify();
            }))
            .child(
                div()
                    .size(px(14.0))
                    .rounded_full()
                    .border_1()
                    .border_color(theme.border_strong)
                    .when(selected, |dot| dot.bg(theme.accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().text_size(px(13.0)).child(label))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child(hint),
                    ),
            )
    }

    fn field_row(label: &'static str, field: &Entity<TextField>) -> bezel::gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(bezel::theme::ink(0.55))
                    .child(label),
            )
            .child(field.clone())
    }

    fn status_line(&self, cx: &Context<Self>) -> bezel::gpui::AnyElement {
        let theme = Theme::of(cx).clone();
        let (text, color): (&str, bezel::gpui::Hsla) = match &self.test {
            TestState::Idle => ("", theme.text_faint),
            TestState::Testing => ("testing…", theme.warning),
            TestState::Ok => ("connection ok", theme.success),
            TestState::Failed(e) => (e.as_str(), theme.danger),
        };
        div()
            .min_h(px(18.0))
            .text_size(px(12.0))
            .text_color(color)
            .child(SharedString::from(text.to_string()))
            .into_any_element()
    }
}

impl Render for ConnectFlow {
    fn render(
        &mut self,
        _window: &mut bezel::gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        let theme = Theme::of(cx).clone();
        let fields = match self.mode {
            Mode::Cloud => div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(Self::field_row("Base URL", &self.base_url))
                .child(Self::field_row("API key", &self.api_key)),
            Mode::ClickHouse => div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(Self::field_row("URL", &self.url))
                .child(Self::field_row("User", &self.user))
                .child(Self::field_row("Password", &self.password)),
            Mode::Postgres => div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(Self::field_row("URL", &self.url))
                .child(Self::field_row("User", &self.user))
                .child(Self::field_row("Password", &self.password))
                .child(Self::field_row("Database", &self.database)),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .max_w(px(460.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().text_size(px(16.0)).child("Add a host"))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child("ClickHouse, Postgres, or the chmonitor cloud API."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(self.mode_row(
                        &theme,
                        "Cloud",
                        "chmonitor-hosted dashboard API · base URL + API key",
                        Mode::Cloud,
                        cx,
                    ))
                    .child(self.mode_row(
                        &theme,
                        "ClickHouse",
                        "HTTP endpoint · url + user + password",
                        Mode::ClickHouse,
                        cx,
                    ))
                    .child(self.mode_row(
                        &theme,
                        "Postgres",
                        "libpq endpoint · url + user + password + database",
                        Mode::Postgres,
                        cx,
                    )),
            )
            .child(fields)
            .child(Self::field_row("Name", &self.name))
            .child(self.status_line(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.0))
                    .child(
                        theme
                            .button("Test", ButtonStyle::Ghost, None)
                            .id("test")
                            .on_click(cx.listener(|this, _, _, cx| this.run_test(cx))),
                    )
                    .child(
                        theme
                            .button("Save", ButtonStyle::Prominent, None)
                            .id("save")
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
    }
}
