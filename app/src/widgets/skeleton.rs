//! Dashboard-style skeleton placeholders (gpui-component Skeleton).

use gpui::{App, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, skeleton::Skeleton};

use crate::density::Density;

fn bone(w: f32, h: f32) -> Skeleton {
    Skeleton::new().w(px(w)).h(px(h)).rounded_md()
}

pub fn metric_grid(cx: &App) -> impl IntoElement {
    let border = cx.theme().border;
    let radius = cx.theme().radius;
    let d = Density::current();
    let per_row = d.metrics_per_row();
    let rows = 6usize.div_ceil(per_row);
    div()
        .flex()
        .flex_col()
        .gap(px(d.card_gap()))
        .w_full()
        .children((0..rows).map(move |_| {
            div()
                .flex()
                .flex_row()
                .gap(px(d.card_gap()))
                .children((0..per_row).map(move |_| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p(px(d.card_pad()))
                        .flex_1()
                        .min_w(px(d.card_min_w()))
                        .border_1()
                        .border_color(border)
                        .rounded(radius)
                        .child(bone(64., 8.))
                        .child(bone(80., 16.))
                }))
        }))
}

pub fn chart_block(cx: &App) -> impl IntoElement {
    let d = Density::current();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .w_full()
        .h(px(d.chart_h()))
        .p(px(d.card_pad()))
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(bone(96., 10.))
        .child(div().flex_1().w_full().child(bone(360., d.chart_h() - 36.)))
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
