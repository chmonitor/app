//! Metric card: a labeled headline number with an optional sub-line.

use gpui::{App, FontWeight, div, prelude::*, px};
use gpui_component::ActiveTheme as _;

/// A stat tile: small muted label, large value, optional muted sub-line.
pub fn metric_card(label: &str, value: &str, sub: Option<&str>, cx: &App) -> impl IntoElement {
    let label = label.to_string();
    let value = value.to_string();
    let sub = sub.map(str::to_string);

    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(12.))
        .min_w(px(140.))
        .flex_1()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(value),
        )
        .children(sub.map(|sub| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(sub)
        }))
}
