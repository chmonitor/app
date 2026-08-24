//! Traffic charts (qps, rows, network).

use chm_core::TrafficSeries;

use gpui::{App, Context, Render, Window, div, prelude::*, px};

use crate::pages::status;
use crate::widgets::{NamedSeries, line_chart};

pub struct TrafficPage {
    data: Option<TrafficSeries>,
    error: Option<String>,
}

impl Default for TrafficPage {
    fn default() -> Self {
        Self::new()
    }
}

impl TrafficPage {
    pub fn new() -> Self {
        Self {
            data: None,
            error: None,
        }
    }

    pub fn set(&mut self, data: Result<TrafficSeries, String>, cx: &mut Context<Self>) {
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

fn chart(title: &str, unit: &str, points: &[chm_core::SeriesPoint], cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .h(px(200.))
        .child(line_chart(
            title,
            unit,
            vec![NamedSeries {
                name: title.into(),
                points: points.to_vec(),
                accent: true,
            }],
            cx,
        ))
}

impl Render for TrafficPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(err) = &self.error {
            return status(format!("traffic unavailable: {err}"), cx).into_any_element();
        }
        let Some(t) = &self.data else {
            return status("loading traffic…", cx).into_any_element();
        };
        if t.queries_per_sec.is_empty()
            && t.rows_read_per_sec.is_empty()
            && t.network_rx_bps.is_empty()
            && t.network_tx_bps.is_empty()
        {
            return status("no traffic in this range", cx).into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.))
                    .child(chart("queries / sec", "qps", &t.queries_per_sec, cx))
                    .child(chart("rows read / sec", "rows/s", &t.rows_read_per_sec, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.))
                    .child(chart("network in", "bit/s", &t.network_rx_bps, cx))
                    .child(chart("network out", "bit/s", &t.network_tx_bps, cx)),
            )
            .into_any_element()
    }
}
