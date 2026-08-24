//! Dashboard-style skeleton placeholders (gpui-component Skeleton).

use gpui::{App, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, skeleton::Skeleton};

fn bone(w: f32, h: f32) -> Skeleton {
    Skeleton::new().w(px(w)).h(px(h)).rounded_md()
}

pub fn metric_grid(cx: &App) -> impl IntoElement {
    let border = cx.theme().border;
    let radius = cx.theme().radius;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .w_full()
        .children((0..3).map(|_| {
            div().flex().flex_row().gap_3().children((0..4).map(|_| {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .flex_1()
                    .min_w(px(140.))
                    .border_1()
                    .border_color(border)
                    .rounded(radius)
                    .child(bone(72., 10.))
                    .child(bone(96., 22.))
            }))
        }))
}

pub fn chart_block(cx: &App) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .w_full()
        .h(px(200.))
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(bone(120., 12.))
        .child(div().flex_1().w_full().child(bone(400., 140.)))
}

pub fn table_block(cx: &App) -> impl IntoElement {
    let border = cx.theme().border;
    div()
        .flex()
        .flex_col()
        .w_full()
        .border_1()
        .border_color(border)
        .rounded(cx.theme().radius)
        .children((0..8).map(|i| {
            div()
                .flex()
                .flex_row()
                .gap_3()
                .px_3()
                .py_2()
                .when(i > 0, |r| r.border_t_1().border_color(border))
                .child(bone(80., 10.))
                .child(bone(140., 10.))
                .child(bone(60., 10.))
                .child(div().flex_1().child(bone(180., 10.)))
        }))
}
