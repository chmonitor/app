//! Metric card: a labeled headline number with an optional sub-line, painted
//! from the theme tokens without needing an `App` in scope.

use bezel::gpui::{div, prelude::*, px};
use bezel::theme::{Theme, current_appearance};

/// Resolve the theme without a context: the process-wide appearance mirror
/// plus the palette builder give exactly what `Theme::of(cx)` would return.
pub(crate) fn theme_now() -> Theme {
    Theme::for_appearance(current_appearance())
}

/// A stat tile: small muted label, large value, optional muted sub-line.
pub fn metric_card(label: &str, value: &str, sub: Option<&str>) -> impl IntoElement {
    let t = theme_now();
    let label = label.to_string();
    let value = value.to_string();
    let sub = sub.map(str::to_string);

    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .p(px(12.0))
        .min_w(px(140.0))
        .flex_1()
        .bg(t.surface_card)
        .border_1()
        .border_color(t.border)
        .rounded(px(Theme::PANEL_RADIUS))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(t.text_muted)
                .child(label),
        )
        .child(div().text_size(px(20.0)).text_color(t.text).child(value))
        .children(sub.map(|sub| {
            div()
                .text_size(px(11.0))
                .text_color(t.text_faint)
                .child(sub)
        }))
}
