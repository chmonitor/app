//! Cluster health summary.

use chm_core::Health;

use bezel::gpui::{Context, Render, div, prelude::*, px};

use crate::pages::status;
use crate::widgets::geometry::format_duration_ms;
use crate::widgets::metric_card;

pub struct HealthPage {
    data: Option<Health>,
    error: Option<String>,
}

impl Default for HealthPage {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthPage {
    pub fn new() -> Self {
        Self {
            data: None,
            error: None,
        }
    }

    pub fn set(&mut self, data: Result<Health, String>, cx: &mut Context<Self>) {
        match data {
            Ok(v) => {
                self.data = Some(v);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }
}

impl Render for HealthPage {
    fn render(
        &mut self,
        _window: &mut bezel::gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        if let Some(err) = &self.error {
            return status(format!("health unavailable: {err}"));
        }
        let Some(h) = &self.data else {
            return status("loading health…");
        };
        let pool_pct = format!(
            "{:.0}%",
            (h.background_pool_utilization * 100.0).clamp(0.0, 999.0)
        );
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.0))
                    .child(metric_card(
                        "status",
                        if h.ok { "ok" } else { "not ok" },
                        None,
                    ))
                    .child(metric_card(
                        "readonly tables",
                        &h.readonly_tables.to_string(),
                        None,
                    ))
                    .child(metric_card(
                        "replication lag",
                        &format_duration_ms(h.replication_lag_max_sec * 1000.0),
                        None,
                    ))
                    .child(metric_card(
                        "zookeeper",
                        if h.zookeeper_available {
                            "available"
                        } else {
                            "unavailable"
                        },
                        None,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.0))
                    .child(metric_card(
                        "delayed inserts",
                        &h.delayed_inserts.to_string(),
                        None,
                    ))
                    .child(metric_card(
                        "distributed files",
                        &h.distributed_files_to_insert.to_string(),
                        None,
                    ))
                    .child(metric_card("background pool", &pool_pct, None)),
            )
    }
}
