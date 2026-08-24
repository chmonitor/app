//! Settings page — appearance, update channel, telemetry. Opened from the
//! app menu (`cmd-,`) or the sidebar footer. Writes `[ui]` / `[telemetry]`
//! / `profile.channel` in config.toml.

use chm_update::Channel;
use gpui::{App, Context, Render, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Sizable as _, Theme, ThemeMode, checkbox::Checkbox, h_flex, v_flex,
};

use crate::config::{config_path, load_config, save_config};
use crate::density::{Density, OverviewMetric, default_metric_ids};
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
    density: Density,
    overview_metrics: Vec<String>,
    show_chart: bool,
    compact_sidebar: bool,
    show_perf: bool,
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
        let overview_metrics = if cfg.ui.overview_metrics.is_empty() {
            default_metric_ids()
        } else {
            cfg.ui.overview_metrics.clone()
        };
        Self {
            appearance: appearance_from_cfg(cfg.ui.appearance.as_deref()),
            density: Density::from_cfg(cfg.ui.density.as_deref()),
            overview_metrics,
            show_chart: cfg.ui.show_chart,
            compact_sidebar: cfg.ui.compact_sidebar,
            show_perf: cfg.ui.show_perf,
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
        cfg.ui.density = Some(self.density.as_str().into());
        cfg.ui.overview_metrics = self.overview_metrics.clone();
        cfg.ui.show_chart = self.show_chart;
        cfg.ui.compact_sidebar = self.compact_sidebar;
        cfg.ui.show_perf = self.show_perf;
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

    fn set_density(&mut self, density: Density, window: &mut Window, cx: &mut Context<Self>) {
        self.density = density;
        self.persist();
        crate::theme::apply_brand(cx);
        window.refresh();
        cx.notify();
    }

    fn set_metric(&mut self, metric: OverviewMetric, on: bool, cx: &mut Context<Self>) {
        let id = metric.id();
        if on {
            if !self.overview_metrics.iter().any(|s| s == id) {
                self.overview_metrics.push(id.into());
            }
        } else {
            self.overview_metrics.retain(|s| s != id);
            if self.overview_metrics.is_empty() {
                self.overview_metrics = default_metric_ids();
            }
        }
        self.persist();
        cx.notify();
    }

    fn set_show_chart(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.show_chart = enabled;
        self.persist();
        cx.notify();
    }

    fn set_compact_sidebar(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.compact_sidebar = enabled;
        self.persist();
        cx.notify();
    }

    fn set_show_perf(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.show_perf = enabled;
        self.persist();
        cx.notify();
    }

    fn metric_on(&self, metric: OverviewMetric) -> bool {
        self.overview_metrics.iter().any(|s| s == metric.id())
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path = config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no config directory)".into());
        let appearance = self.appearance;
        let density = self.density;
        let show_chart = self.show_chart;
        let compact_sidebar = self.compact_sidebar;
        let show_perf = self.show_perf;
        let channel = self.channel;
        let telemetry = self.telemetry;
        let update_enabled = self.update_enabled;
        let auto_download = self.auto_download;

        v_flex()
            .gap_5()
            .max_w(px(520.))
            .child(heading("Appearance", cx))
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
            .child(heading("Density", cx))
            .child({
                let entity = cx.entity().downgrade();
                let mut group = radio_group("density");
                for &mode in &Density::ALL {
                    let entity = entity.clone();
                    group = group.child(
                        choice_radio(
                            format!("den-{}", mode.as_str()),
                            density == mode,
                            mode.label(),
                            mode.hint(),
                            cx,
                        )
                        .on_change(move |next, _, window, cx| {
                            if next {
                                let _ = entity.update(cx, |this, cx| {
                                    this.set_density(mode, window, cx);
                                });
                            }
                        }),
                    );
                }
                group
            })
            .child(heading("Overview metrics", cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("tiles on Overview — defaults to Active Queries, Schema, Storage, Uptime"),
            )
            .child({
                let entity = cx.entity().downgrade();
                let mut cols = h_flex().gap_4().items_start();
                for column in OverviewMetric::ALL.chunks(6) {
                    let mut col = v_flex().gap_2().flex_1();
                    for &metric in column {
                        let on = self.metric_on(metric);
                        let entity = entity.clone();
                        col = col.child(
                            Checkbox::new(format!("m-{}", metric.id()))
                                .label(metric.label())
                                .checked(on)
                                .small()
                                .on_click(move |next, _, cx| {
                                    let on = *next;
                                    let _ = entity.update(cx, |this, cx| {
                                        this.set_metric(metric, on, cx);
                                    });
                                }),
                        );
                    }
                    cols = cols.child(col);
                }
                cols
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
                            .child(div().text_sm().child("Queries / sec chart"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("sparkline under the metric tiles"),
                            ),
                    )
                    .child(theme_switch("show-chart", show_chart, cx).on_click(
                        move |next, _, cx| {
                            let on = *next;
                            let _ = entity.update(cx, |this, cx| this.set_show_chart(on, cx));
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
                            .child(div().text_sm().child("Compact sidebar"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("start with the icon strip (⌘B still toggles)"),
                            ),
                    )
                    .child(theme_switch("compact-sidebar", compact_sidebar, cx).on_click(
                        move |next, _, cx| {
                            let on = *next;
                            let _ =
                                entity.update(cx, |this, cx| this.set_compact_sidebar(on, cx));
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
                            .child(div().text_sm().child("Status bar timing"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("fetch latency and memory in the status bar"),
                            ),
                    )
                    .child(theme_switch("show-perf", show_perf, cx).on_click(
                        move |next, _, cx| {
                            let on = *next;
                            let _ = entity.update(cx, |this, cx| this.set_show_perf(on, cx));
                        },
                    ))
            })
            .child(heading("Updates", cx))
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
                    .child(theme_switch("upd-enabled", update_enabled, cx).on_click(
                        move |next, _, cx| {
                            let on = *next;
                            let _ = entity.update(cx, |this, cx| this.set_update_enabled(on, cx));
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
                    .child(theme_switch("upd-auto", auto_download, cx).on_click(
                        move |next, _, cx| {
                            let on = *next;
                            let _ = entity.update(cx, |this, cx| this.set_auto_download(on, cx));
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
            .child(heading("Telemetry", cx))
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
                    .child(theme_switch("telemetry", telemetry, cx).on_click(
                        move |next, _, cx| {
                            let on = *next;
                            let _ = entity.update(cx, |this, cx| this.set_telemetry(on, cx));
                        },
                    ))
            })
            .child(heading("Shortcuts", cx))
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
            .child(heading("Config file", cx))
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
