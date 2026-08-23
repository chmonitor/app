//! Overview page — placeholder-level metric card grid.
//! AGENT D owns this stub so smoke shots have content; Agents F/G/H replace
//! pages/ wholesale later. Renders whatever `Shell` fetched (mock data under
//! CHM_SMOKE=1).

use chm_core::Overview;

use bezel::gpui::{Context, Render, div, prelude::*, px};

pub struct OverviewPage {
    data: Option<Overview>,
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
            error: None,
        }
    }

    pub fn set_overview(&mut self, data: Result<Overview, String>, cx: &mut Context<Self>) {
        match data {
            Ok(o) => {
                self.data = Some(o);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn card(label: &'static str, value: String) -> bezel::gpui::Div {
        div()
            .w(px(180.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(12.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(bezel::theme::ink(0.10))
            .bg(bezel::theme::ink(0.03))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(bezel::theme::ink(0.55))
                    .child(label),
            )
            .child(div().text_size(px(17.0)).child(value))
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
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(bezel::theme::ink(0.55))
                .text_size(px(13.0))
                .child(format!("overview unavailable: {err}"));
        }
        let Some(o) = &self.data else {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(bezel::theme::ink(0.45))
                .text_size(px(13.0))
                .child("loading overview…");
        };

        let rows: [[(&'static str, String); 4]; 3] = [
            [
                ("queries / sec", format!("{:.1}", o.qps)),
                ("running", o.running_queries.to_string()),
                ("slow · 24h", o.slow_queries_24h.to_string()),
                ("failed · 24h", o.failed_queries_24h.to_string()),
            ],
            [
                ("active merges", o.active_merges.to_string()),
                (
                    "replicas",
                    format!("{} / {}", o.replicas_ok, o.replicas_total),
                ),
                ("tables", o.tables_total.to_string()),
                ("parts", fmt_u64(o.parts_total)),
            ],
            [
                ("disk used", fmt_bytes(o.disk_used_bytes)),
                (
                    "disk total",
                    format!(
                        "{} ({:.0}% used)",
                        fmt_bytes(o.disk_total_bytes),
                        100.0 * o.disk_used_bytes as f64 / o.disk_total_bytes.max(1) as f64
                    ),
                ),
                ("uptime", fmt_duration(o.uptime_seconds)),
                ("version", o.clickhouse_version.clone()),
            ],
        ];

        let mut grid = div().flex().flex_col().gap(px(10.0));
        for row in rows {
            let mut line = div().flex().flex_row().gap(px(10.0));
            for (label, value) in row {
                line = line.child(Self::card(label, value));
            }
            grid = grid.child(line);
        }
        grid
    }
}

/// Thousands separators, no external crate.
fn fmt_u64(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push('\'');
        }
        out.push(*b as char);
    }
    out
}

fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn fmt_duration(secs: u64) -> String {
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    match (d, h) {
        (0, 0) => format!("{}m", secs / 60),
        (0, h) => format!("{h}h"),
        (d, h) => format!("{d}d {h}h"),
    }
}
