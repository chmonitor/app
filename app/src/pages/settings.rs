//! Settings page — appearance, update channel, telemetry. Opened from the
//! app menu (`cmd-,`) or the sidebar footer. Writes `[ui]` / `[telemetry]`
//! / `profile.channel` in config.toml.

use bezel::gpui::{Context, Render, SharedString, Window, div, prelude::*, px};
use bezel::theme::{Theme, appearance::AppearanceMode};
use chm_update::Channel;

use crate::config::{config_path, load_config, save_config};
use crate::pages::heading;

pub struct SettingsPage {
    appearance: AppearanceMode,
    channel: Channel,
    telemetry: bool,
    status: Option<String>,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPage {
    pub fn new() -> Self {
        let cfg = load_config();
        Self {
            appearance: appearance_from_cfg(cfg.ui.appearance.as_deref()),
            channel: channel_from_cfg(cfg.profile.channel.as_deref()),
            telemetry: cfg.telemetry.enabled,
            status: None,
        }
    }

    fn persist(&mut self) {
        let mut cfg = load_config();
        cfg.ui.appearance = Some(appearance_to_cfg(self.appearance).into());
        cfg.profile.channel = Some(self.channel.as_str().into());
        cfg.telemetry.enabled = self.telemetry;
        self.status = save_config(&cfg).err();
    }

    fn set_appearance(&mut self, mode: AppearanceMode, cx: &mut Context<Self>) {
        self.appearance = mode;
        bezel::theme::appearance::set_mode(mode, cx);
        self.persist();
        cx.notify();
    }

    fn set_channel(&mut self, channel: Channel, cx: &mut Context<Self>) {
        self.channel = channel;
        self.persist();
        cx.notify();
    }

    fn set_telemetry(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.telemetry = enabled;
        self.persist();
        cx.notify();
    }

    fn choice_row(
        &self,
        theme: &Theme,
        id: &'static str,
        title: (&'static str, &'static str),
        selected: bool,
        on: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        let (label, hint) = title;
        div()
            .id(SharedString::from(id))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(8.0))
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
            .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
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

    fn section(
        title: &str,
        children: impl IntoIterator<Item = bezel::gpui::AnyElement>,
    ) -> bezel::gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(heading(title))
            .children(children)
    }
}

impl Render for SettingsPage {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        let theme = Theme::of(cx).clone();
        let path = config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no config directory)".into());
        let appearance = self.appearance;
        let channel = self.channel;
        let telemetry = self.telemetry;

        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .max_w(px(520.0))
            .child(Self::section(
                "Appearance",
                AppearanceMode::ALL.iter().map(|&mode| {
                    self.choice_row(
                        &theme,
                        match mode {
                            AppearanceMode::System => "app-system",
                            AppearanceMode::Light => "app-light",
                            AppearanceMode::Dark => "app-dark",
                        },
                        (
                            mode.label(),
                            match mode {
                                AppearanceMode::System => "follow macOS light/dark",
                                AppearanceMode::Light => "always light",
                                AppearanceMode::Dark => "always dark",
                            },
                        ),
                        appearance == mode,
                        move |this, cx| this.set_appearance(mode, cx),
                        cx,
                    )
                    .into_any_element()
                }),
            ))
            .child(Self::section(
                "Updates",
                [
                    self.choice_row(
                        &theme,
                        "ch-stable",
                        ("Stable", "tagged releases"),
                        channel == Channel::Stable,
                        |this, cx| this.set_channel(Channel::Stable, cx),
                        cx,
                    )
                    .into_any_element(),
                    self.choice_row(
                        &theme,
                        "ch-beta",
                        ("Beta", "pre-release builds"),
                        channel == Channel::Beta,
                        |this, cx| this.set_channel(Channel::Beta, cx),
                        cx,
                    )
                    .into_any_element(),
                ],
            ))
            .child(Self::section(
                "Telemetry",
                [
                    self.choice_row(
                        &theme,
                        "tel-off",
                        ("Off", "nothing is recorded or sent (default)"),
                        !telemetry,
                        |this, cx| this.set_telemetry(false, cx),
                        cx,
                    )
                    .into_any_element(),
                    self.choice_row(
                        &theme,
                        "tel-on",
                        ("On", "local fetch timings only; no query text"),
                        telemetry,
                        |this, cx| this.set_telemetry(true, cx),
                        cx,
                    )
                    .into_any_element(),
                ],
            ))
            .child(Self::section(
                "Shortcuts",
                [div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child("1–8   switch page")
                    .child("r     refresh")
                    .child("⌘B    toggle sidebar")
                    .child("⌘,    settings")
                    .child("⌘Q    quit")
                    .into_any_element()],
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(heading("Config file"))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child(format!("chmonitor {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child(path),
                    ),
            )
            .children(self.status.as_ref().map(|e| {
                div()
                    .text_size(px(12.0))
                    .text_color(theme.danger)
                    .child(e.clone())
            }))
    }
}

pub fn appearance_from_cfg(s: Option<&str>) -> AppearanceMode {
    match s.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("light") => AppearanceMode::Light,
        Some("dark") => AppearanceMode::Dark,
        _ => AppearanceMode::System,
    }
}

fn appearance_to_cfg(mode: AppearanceMode) -> &'static str {
    match mode {
        AppearanceMode::System => "system",
        AppearanceMode::Light => "light",
        AppearanceMode::Dark => "dark",
    }
}

fn channel_from_cfg(s: Option<&str>) -> Channel {
    match s.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("beta") => Channel::Beta,
        _ => Channel::Stable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_and_channel_parse() {
        assert_eq!(appearance_from_cfg(None), AppearanceMode::System);
        assert_eq!(appearance_from_cfg(Some("DARK")), AppearanceMode::Dark);
        assert_eq!(appearance_from_cfg(Some("light")), AppearanceMode::Light);
        assert_eq!(channel_from_cfg(Some("beta")), Channel::Beta);
        assert_eq!(channel_from_cfg(None), Channel::Stable);
        assert_eq!(appearance_to_cfg(AppearanceMode::Dark), "dark");
    }
}
