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
        .gap(px(6.))
        .p(px(14.))
        .min_w(px(140.))
        .flex_1()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_size(px(26.))
                .font_weight(FontWeight::BOLD)
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
