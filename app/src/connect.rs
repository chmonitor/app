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

use crate::config::{ProfileConfig, config_path, source_from_profile};

/// Fired after Save successfully writes config.toml.
#[derive(Debug, Clone)]
pub enum ConnectEvent {
    SavedProfile(ProfileConfig),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Cloud,
    Direct,
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
            Some("clickhouse") => Mode::Direct,
            _ => Mode::Cloud,
        };
        Self {
            focus: cx.focus_handle(),
            mode,
            base_url: field(initial.base_url, "https://acme.dash.chmonitor.dev", cx),
            api_key: field(initial.api_key, "API key", cx),
            url: field(initial.url, "http://localhost:8123", cx),
            user: field(initial.user.or(Some("default".into())), "user", cx),
            password: field(initial.password, "password", cx),
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
            Mode::Direct => {
                let url = nonempty(Self::read(&self.url, cx))?;
                Some(ProfileConfig {
                    mode: Some("clickhouse".into()),
                    url: Some(url),
                    user: nonempty(Self::read(&self.user, cx)),
                    password: nonempty(Self::read(&self.password, cx)),
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
        // Preserve anything outside [profile] (e.g. [telemetry]) if a config
        // already exists; otherwise start from defaults.
        let mut cfg = config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| toml::from_str::<crate::config::ConfigFile>(&text).ok())
            .unwrap_or_default();
        cfg.profile = profile.clone();
        let out = match toml::to_string_pretty(&cfg) {
            Ok(out) => out,
            Err(e) => {
                self.test = TestState::Failed(format!("serialize failed: {e}"));
                cx.notify();
                return;
            }
        };
        let Some(path) = config_path() else {
            self.test = TestState::Failed("no config directory on this platform".into());
            cx.notify();
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            self.test = TestState::Failed(format!("mkdir failed: {e}"));
            cx.notify();
            return;
        }
        match std::fs::write(&path, out) {
            Ok(()) => {
                self.test = TestState::Ok;
                cx.emit(ConnectEvent::SavedProfile(profile));
                cx.notify();
            }
            Err(e) => {
                self.test = TestState::Failed(format!("write failed: {e}"));
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
        let (cloud_fields, direct_fields) = match self.mode {
            Mode::Cloud => (
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(Self::field_row("Base URL", &self.base_url))
                    .child(Self::field_row("API key", &self.api_key)),
                div(),
            ),
            Mode::Direct => (
                div(),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(Self::field_row("URL", &self.url))
                    .child(Self::field_row("User", &self.user))
                    .child(Self::field_row("Password", &self.password)),
            ),
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
                    .child(div().text_size(px(16.0)).child("Connect to ClickHouse"))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child("Use the chmonitor cloud API or talk to your server directly."),
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
                        "Direct",
                        "ClickHouse HTTP endpoint · url + user + password",
                        Mode::Direct,
                        cx,
                    )),
            )
            .child(cloud_fields)
            .child(direct_fields)
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
