//! Settings page — appearance, update channel, telemetry. Opened from the
//! app menu (`cmd-,`) or the sidebar footer. Writes `[ui]` / `[telemetry]`
//! / `profile.channel` in config.toml.

use chm_update::Channel;
use gpui::{App, Context, Render, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Theme, ThemeMode, h_flex, v_flex};

use crate::config::{config_path, load_config, save_config};
use crate::pages::heading;
use crate::widgets::controls::{choice_radio, radio_group, theme_switch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::System => "follow macOS light/dark",
            Self::Light => "always light",
            Self::Dark => "always dark",
        }
    }
}

pub struct SettingsPage {
    appearance: Appearance,
    channel: Channel,
    telemetry: bool,
    update_enabled: bool,
    auto_download: bool,
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
            update_enabled: cfg.update.enabled,
            auto_download: cfg.update.auto_download,
            status: None,
        }
    }

    fn persist(&mut self) {
        let mut cfg = load_config();
        cfg.ui.appearance = Some(appearance_to_cfg(self.appearance).into());
        cfg.profile.channel = Some(self.channel.as_str().into());
        cfg.telemetry.enabled = self.telemetry;
        cfg.update.enabled = self.update_enabled;
        cfg.update.auto_download = self.auto_download;
        self.status = save_config(&cfg).err();
    }

    fn set_appearance(&mut self, mode: Appearance, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance = mode;
        apply_appearance(mode, window, cx);
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

    fn set_update_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.update_enabled = enabled;
        self.persist();
        cx.notify();
    }

    fn set_auto_download(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.auto_download = enabled;
        self.persist();
        cx.notify();
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path = config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no config directory)".into());
        let appearance = self.appearance;
        let channel = self.channel;
        let telemetry = self.telemetry;
        let update_enabled = self.update_enabled;
        let auto_download = self.auto_download;

        v_flex()
            .gap_5()
            .max_w(px(520.))
            .child(heading("Appearance"))
            .child({
                let entity = cx.entity().downgrade();
                let mut group = radio_group("appearance");
                for &mode in &Appearance::ALL {
                    let entity = entity.clone();
                    group = group.child(
                        choice_radio(
                            format!("app-{}", appearance_to_cfg(mode)),
                            appearance == mode,
                            mode.label(),
                            mode.hint(),
                            cx,
                        )
                        .on_change(move |next, _, window, cx| {
                            if next {
                                let _ = entity.update(cx, |this, cx| {
                                    this.set_appearance(mode, window, cx);
                                });
                            }
                        }),
                    );
                }
                group
            })
            .child(heading("Updates"))
            .child({
                let entity = cx.entity().downgrade();
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().child("Check on launch"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("fetch the channel manifest from updates.chmonitor.dev"),
                            ),
                    )
                    .child(theme_switch("upd-enabled", update_enabled, cx).on_change(
                        move |next, _, _, cx| {
                            let _ = entity.update(cx, |this, cx| this.set_update_enabled(next, cx));
                        },
                    ))
            })
            .child({
                let entity = cx.entity().downgrade();
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().child("Download automatically"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("save the archive when a newer build is found"),
                            ),
                    )
                    .child(theme_switch("upd-auto", auto_download, cx).on_change(
                        move |next, _, _, cx| {
                            let _ = entity.update(cx, |this, cx| this.set_auto_download(next, cx));
                        },
                    ))
            })
            .child({
                let entity = cx.entity().downgrade();
                radio_group("channel")
                    .child(
                        choice_radio(
                            "ch-stable",
                            channel == Channel::Stable,
                            "Stable",
                            "tagged releases",
                            cx,
                        )
                        .on_change({
                            let entity = entity.clone();
                            move |next, _, _, cx| {
                                if next {
                                    let _ = entity.update(cx, |this, cx| {
                                        this.set_channel(Channel::Stable, cx);
                                    });
                                }
                            }
                        }),
                    )
                    .child(
                        choice_radio(
                            "ch-beta",
                            channel == Channel::Beta,
                            "Beta",
                            "pre-release builds",
                            cx,
                        )
                        .on_change(move |next, _, _, cx| {
                            if next {
                                let _ = entity.update(cx, |this, cx| {
                                    this.set_channel(Channel::Beta, cx);
                                });
                            }
                        }),
                    )
            })
            .child(heading("Telemetry"))
            .child({
                let entity = cx.entity().downgrade();
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_sm().child("Anonymous usage"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "install ping + page views to telemetry.chmonitor.dev; no SQL or hostnames",
                                    ),
                            ),
                    )
                    .child(theme_switch("telemetry", telemetry, cx).on_change(
                        move |next, _, _, cx| {
                            let _ = entity.update(cx, |this, cx| this.set_telemetry(next, cx));
                        },
                    ))
            })
            .child(heading("Shortcuts"))
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("1–8   switch page")
                    .child("r     refresh")
                    .child("⌘B    toggle sidebar")
                    .child("⌘,    settings")
                    .child("⌘Q    quit"),
            )
            .child(heading("Config file"))
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("chmonitor {}", env!("CARGO_PKG_VERSION")))
                    .child(path),
            )
            .children(self.status.as_ref().map(|e| {
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(e.clone())
            }))
    }
}

pub fn apply_appearance(mode: Appearance, window: &mut Window, cx: &mut App) {
    match mode {
        Appearance::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        Appearance::System => Theme::sync_system_appearance(Some(window), cx),
    }
    crate::theme::apply_brand(cx);
}

pub fn appearance_from_cfg(s: Option<&str>) -> Appearance {
    match s.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

pub(crate) fn appearance_to_cfg(mode: Appearance) -> &'static str {
    match mode {
        Appearance::System => "system",
        Appearance::Light => "light",
        Appearance::Dark => "dark",
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
        assert_eq!(appearance_from_cfg(None), Appearance::System);
        assert_eq!(appearance_from_cfg(Some("DARK")), Appearance::Dark);
        assert_eq!(appearance_from_cfg(Some("light")), Appearance::Light);
        assert_eq!(channel_from_cfg(Some("beta")), Channel::Beta);
        assert_eq!(channel_from_cfg(None), Channel::Stable);
        assert_eq!(appearance_to_cfg(Appearance::Dark), "dark");
    }
}
