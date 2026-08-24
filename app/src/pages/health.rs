//! Cluster health summary.

use chm_core::Health;

use gpui::{Context, Render, Window, div, prelude::*, px};

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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(err) = &self.error {
            return status(format!("health unavailable: {err}"), cx).into_any_element();
        }
        let Some(h) = &self.data else {
            return crate::widgets::skeleton::metric_grid(cx).into_any_element();
        };
        let pool_pct = format!(
            "{:.0}%",
            (h.background_pool_utilization * 100.0).clamp(0.0, 999.0)
        );
        let gap = px(crate::density::Density::current().card_gap());
        div()
            .flex()
            .flex_col()
            .gap(gap)
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(gap)
                    .child(metric_card(
                        "status",
                        if h.ok { "ok" } else { "not ok" },
                        None,
                        cx,
                    ))
                    .child(metric_card(
                        "readonly tables",
                        &h.readonly_tables.to_string(),
                        None,
                        cx,
                    ))
                    .child(metric_card(
                        "replication lag",
                        &format_duration_ms(h.replication_lag_max_sec * 1000.0),
                        None,
                        cx,
                    ))
                    .child(metric_card(
                        "zookeeper",
                        if h.zookeeper_available {
                            "available"
                        } else {
                            "unavailable"
                        },
                        None,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(gap)
                    .child(metric_card(
                        "delayed inserts",
                        &h.delayed_inserts.to_string(),
                        None,
                        cx,
                    ))
                    .child(metric_card(
                        "distributed files",
                        &h.distributed_files_to_insert.to_string(),
                        None,
                        cx,
                    ))
                    .child(metric_card("background pool", &pool_pct, None, cx)),
            )
            .into_any_element()
    }
}
