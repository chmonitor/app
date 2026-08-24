//! Overview page — metric cards plus a queries/sec sparkline.
//! Renders whatever `Shell` fetched (mock data under CHM_SMOKE=1).

use chm_core::{Overview, TrafficSeries};

use bezel::gpui::{Context, Render, div, prelude::*, px};

use crate::pages::status;
use crate::widgets::geometry::{format_bytes, format_count};
use crate::widgets::{NamedSeries, line_chart, metric_card};

pub struct OverviewPage {
    data: Option<Overview>,
    traffic: Option<TrafficSeries>,
    error: Option<String>,
}

impl Default for OverviewPage {
    fn default() -> Self {
        Self::new()
    }
}

impl OverviewPage {
    pub fn new() -> Self {
        Self {
            data: None,
            traffic: None,
            error: None,
        }
    }

    pub fn set_overview(
        &mut self,
        data: Result<Overview, String>,
        traffic: Result<TrafficSeries, String>,
        cx: &mut Context<Self>,
    ) {
        match data {
            Ok(o) => {
                self.data = Some(o);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        if let Ok(t) = traffic {
            self.traffic = Some(t);
        }
        cx.notify();
    }
}

impl Render for OverviewPage {
    fn render(
        &mut self,
        _window: &mut bezel::gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        let _ = cx;
        if let Some(err) = &self.error {
            return status(format!("overview unavailable: {err}"));
        }
        let Some(o) = &self.data else {
            return status("loading overview…");
        };

        let used_pct = 100.0 * o.disk_used_bytes as f64 / o.disk_total_bytes.max(1) as f64;
        let disk_sub = format!(
            "{} · {:.0}% used",
            format_bytes(o.disk_total_bytes),
            used_pct
        );

        let mut grid = div().flex().flex_col().gap(px(10.0)).w_full();
        grid = grid.child(
            div()
                .flex()
                .flex_row()
                .gap(px(10.0))
                .child(metric_card("queries / sec", &format!("{:.1}", o.qps), None))
                .child(metric_card("running", &o.running_queries.to_string(), None))
                .child(metric_card(
                    "slow · 24h",
                    &o.slow_queries_24h.to_string(),
                    None,
                ))
                .child(metric_card(
                    "failed · 24h",
                    &o.failed_queries_24h.to_string(),
                    None,
                )),
        );
        grid = grid.child(
            div()
                .flex()
                .flex_row()
                .gap(px(10.0))
                .child(metric_card(
                    "active merges",
                    &o.active_merges.to_string(),
                    None,
                ))
                .child(metric_card(
                    "replicas",
                    &format!("{} / {}", o.replicas_ok, o.replicas_total),
                    None,
                ))
                .child(metric_card(
                    "tables",
                    &format_count(o.tables_total as f64),
                    None,
                ))
                .child(metric_card(
                    "parts",
                    &format_count(o.parts_total as f64),
                    None,
                )),
        );
        grid = grid.child(
            div()
                .flex()
                .flex_row()
                .gap(px(10.0))
                .child(metric_card(
                    "disk used",
                    &format_bytes(o.disk_used_bytes),
                    Some(&disk_sub),
                ))
                .child(metric_card("uptime", &fmt_uptime(o.uptime_seconds), None))
                .child(metric_card("version", &o.clickhouse_version, None)),
        );

        if let Some(t) = &self.traffic
            && !t.queries_per_sec.is_empty()
        {
            grid = grid.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .h(px(220.0))
                    .child(line_chart(
                        "queries / sec",
                        "qps",
                        vec![NamedSeries {
                            name: "qps".into(),
                            points: t.queries_per_sec.clone(),
                            accent: true,
                        }],
                    )),
            );
        }
        grid
    }
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    match (d, h) {
        (0, 0) => format!("{}m", secs / 60),
        (0, h) => format!("{h}h"),
        (d, h) => format!("{d}d {h}h"),
    }
}
