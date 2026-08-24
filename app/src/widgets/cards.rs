//! Metric card: a labeled headline number with an optional sub-line.

use gpui::{App, FontWeight, div, prelude::*, px};
use gpui_component::ActiveTheme as _;

use crate::density::Density;

/// A stat tile: small muted label, large value, optional muted sub-line.
pub fn metric_card(
    label: impl Into<String>,
    value: impl Into<String>,
    sub: Option<String>,
    cx: &App,
) -> impl IntoElement {
    let label = label.into();
    let value = value.into();
    let d = Density::current();

    div()
        .flex()
        .flex_col()
        .gap(px(4.))
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
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_size(px(d.card_value()))
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
