//! Metric card: a labeled headline number with an optional sub-line.
//! KPI variant matches dash.chmonitor.dev overview tiles (uppercase label,
//! unit, optional storage bar).

use gpui::{App, FontWeight, div, prelude::*, px, relative};
use gpui_component::ActiveTheme as _;

use crate::density::Density;

/// A stat tile: small muted label, large value, optional muted sub-line.
pub fn metric_card(
    label: impl Into<String>,
    value: impl Into<String>,
    sub: Option<String>,
    cx: &App,
) -> impl IntoElement {
    kpi_card(label, value, None, sub, None, cx)
}

/// Dashboard-style KPI: `LABEL` → `value unit` → optional bar → sub-line.
pub fn kpi_card(
    label: impl Into<String>,
    value: impl Into<String>,
    unit: Option<&str>,
    sub: Option<String>,
    progress: Option<f32>,
    cx: &App,
) -> impl IntoElement {
    let label = label.into();
    let value = value.into();
    let unit = unit.map(str::to_string);
    let d = Density::current();
    let pct = progress.map(|p| p.clamp(0.0, 100.0));
    let bar_color = match pct {
        Some(p) if p > 90.0 => cx.theme().danger,
        Some(p) if p > 75.0 => cx.theme().warning,
        _ => cx.theme().primary,
    };

    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(d.card_pad()))
        .min_w(px(d.card_min_w()))
        .flex_1()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(d.card_value()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().foreground)
                        .child(value),
                )
                .children(unit.map(|unit| {
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(unit)
                })),
        )
        .children(pct.map(|p| {
            div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(cx.theme().muted)
                .child(
                    div()
                        .h_full()
                        .w(relative(p / 100.0))
                        .rounded_full()
                        .bg(bar_color),
                )
        }))
        .children(sub.map(|sub| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(sub)
        }))
}
