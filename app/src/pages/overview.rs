//! Overview page — the handful of metrics that answer "is this cluster
//! healthy?", plus an optional queries/sec sparkline. Visible tiles and
//! density come from `[ui]` (Settings).

use chm_core::{Overview, TrafficSeries};

use gpui::{Context, Render, Window, div, prelude::*, px};

use crate::config::load_config;
use crate::density::{Density, OverviewMetric, visible_metrics};
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(err) = &self.error {
            return status(format!("overview unavailable: {err}"), cx).into_any_element();
        }
        let Some(o) = &self.data else {
            return crate::widgets::skeleton::metric_grid(cx).into_any_element();
        };

        let ui = load_config().ui;
        let density = Density::from_cfg(ui.density.as_deref());
        let metrics = visible_metrics(&ui.overview_metrics);
        let gap = px(density.card_gap());
        let per_row = density.metrics_per_row();

        let mut grid = div().flex().flex_col().gap(gap).w_full();
        for chunk in metrics.chunks(per_row) {
            let mut row = div().flex().flex_row().gap(gap);
            for metric in chunk {
                row = row.child(tile(*metric, o, cx));
            }
            grid = grid.child(row);
        }

        if ui.show_chart
            && let Some(t) = &self.traffic
            && !t.queries_per_sec.is_empty()
        {
            grid = grid.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .h(px(density.chart_h()))
                    .child(line_chart(
                        "queries / sec",
                        "qps",
                        vec![NamedSeries {
                            name: "qps".into(),
                            points: t.queries_per_sec.clone(),
                            accent: true,
                        }],
                        cx,
                    )),
            );
        }
        grid.into_any_element()
    }
}

fn tile(metric: OverviewMetric, o: &Overview, cx: &gpui::App) -> impl gpui::IntoElement {
    let (label, value, sub) = match metric {
        OverviewMetric::Qps => ("queries / sec", format!("{:.1}", o.qps), None),
        OverviewMetric::Running => ("running", o.running_queries.to_string(), None),
        OverviewMetric::Slow => ("slow · 24h", o.slow_queries_24h.to_string(), None),
        OverviewMetric::Failed => ("failed · 24h", o.failed_queries_24h.to_string(), None),
        OverviewMetric::Merges => ("active merges", o.active_merges.to_string(), None),
        OverviewMetric::Replicas => (
            "replicas",
            format!("{} / {}", o.replicas_ok, o.replicas_total),
            None,
        ),
        OverviewMetric::Tables => ("tables", format_count(o.tables_total as f64), None),
        OverviewMetric::Parts => ("parts", format_count(o.parts_total as f64), None),
        OverviewMetric::Disk => {
            let used_pct = 100.0 * o.disk_used_bytes as f64 / o.disk_total_bytes.max(1) as f64;
            (
                "disk used",
                format_bytes(o.disk_used_bytes),
                Some(format!(
                    "{} · {:.0}% used",
                    format_bytes(o.disk_total_bytes),
                    used_pct
                )),
            )
        }
        OverviewMetric::Uptime => ("uptime", fmt_uptime(o.uptime_seconds), None),
        OverviewMetric::Version => ("version", o.clickhouse_version.clone(), None),
    };
    metric_card(label, value, sub, cx)
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
