//! Settings page — appearance, update channel, telemetry. Opened from the
//! app menu (`cmd-,`) or the sidebar footer. Writes `[ui]` / `[telemetry]`
//! / `profile.channel` in config.toml.

use chm_update::Channel;
use gpui::{App, Context, Render, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Theme, ThemeMode,
    radio::{Radio, RadioGroup},
    v_flex,
};

use crate::config::{config_path, load_config, save_config};
use crate::pages::heading;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    fn index(self) -> usize {
        Self::ALL.iter().position(|&m| m == self).unwrap_or(0)
    }

    fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Appearance::System)
    }

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
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path = config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no config directory)".into());
        let appearance = self.appearance;
        let channel = self.channel;
        let telemetry = self.telemetry;

        v_flex()
            .gap_5()
            .max_w(px(520.))
            .child(heading("Appearance"))
            .child(
                RadioGroup::vertical("appearance")
                    .children(Appearance::ALL.iter().map(|&mode| {
                        Radio::new(mode.label()).label(mode.label()).child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(mode.hint()),
                        )
                    }))
                    .selected_index(Some(appearance.index()))
                    .on_click(cx.listener(|this, index: &usize, window, cx| {
                        this.set_appearance(Appearance::from_index(*index), window, cx);
                    })),
            )
            .child(heading("Updates"))
            .child(
                RadioGroup::vertical("channel")
                    .child(
                        Radio::new("stable").label("Stable").child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("tagged releases"),
                        ),
                    )
                    .child(
                        Radio::new("beta").label("Beta").child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("pre-release builds"),
                        ),
                    )
                    .selected_index(Some(if channel == Channel::Stable { 0 } else { 1 }))
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.set_channel(
                            if *index == 0 {
                                Channel::Stable
                            } else {
                                Channel::Beta
                            },
                            cx,
                        );
                    })),
            )
            .child(heading("Telemetry"))
            .child(
                RadioGroup::vertical("telemetry")
                    .child(
                        Radio::new("off").label("Off").child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("nothing is recorded or sent (default)"),
                        ),
                    )
                    .child(
                        Radio::new("on").label("On").child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("local fetch timings only; no query text"),
                        ),
                    )
                    .selected_index(Some(if telemetry { 1 } else { 0 }))
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.set_telemetry(*index == 1, cx);
                    })),
            )
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
}

pub fn appearance_from_cfg(s: Option<&str>) -> Appearance {
    match s.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("light") => Appearance::Light,
        Some("dark") => Appearance::Dark,
        _ => Appearance::System,
    }
}

fn appearance_to_cfg(mode: Appearance) -> &'static str {
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
