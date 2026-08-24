//! Time-series chart via gpui-component [`AreaChart`] / [`LineChart`].

use chm_core::SeriesPoint;
use gpui::{
    App, FontWeight, SharedString, div, linear_color_stop, linear_gradient, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    chart::{AreaChart, LineChart},
    h_flex, v_flex,
};

use super::geometry::format_time_of_day;

/// One named line on a chart. `accent` picks the primary chart color.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NamedSeries {
    pub name: String,
    pub points: Vec<SeriesPoint>,
    pub accent: bool,
}

#[derive(Clone)]
struct ChartPt {
    x: String,
    y: f64,
}

fn to_pts(points: &[SeriesPoint]) -> Vec<ChartPt> {
    points
        .iter()
        .map(|p| ChartPt {
            x: format_time_of_day(p.t_ms),
            y: p.v,
        })
        .collect()
}

/// Title + unit + a filled area (accent) or a line (muted). Empty series
/// still render the frame so layout does not jump.
pub fn line_chart(title: &str, unit: &str, series: Vec<NamedSeries>, cx: &App) -> impl IntoElement {
    let title = SharedString::from(title.to_string());
    let unit = SharedString::from(unit.to_string());
    let primary = series.iter().find(|s| s.accent).or(series.first());
    let pts = primary.map(|s| to_pts(&s.points)).unwrap_or_default();
    let accent = primary.map(|s| s.accent).unwrap_or(true);
    let color = if accent {
        cx.theme().chart_1
    } else {
        cx.theme().chart_2
    };
    let tick_margin = (pts.len() / 6).max(1);
    let fill = linear_gradient(
        0.,
        linear_color_stop(color.opacity(0.4), 1.),
        linear_color_stop(cx.theme().background.opacity(0.1), 0.),
    );

    v_flex()
        .gap_1()
        .w_full()
        .h_full()
        .min_h(px(120.))
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .p_3()
        .child(
            h_flex()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(unit),
                ),
        )
        .child(div().flex_1().min_h(px(80.)).w_full().map(|el| {
            if pts.is_empty() {
                el.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("no data"),
                )
            } else if accent {
                el.child(
                    AreaChart::new(pts)
                        .x(|d| d.x.clone())
                        .y(|d| d.y)
                        .stroke(color)
                        .fill(fill)
                        .linear()
                        .tick_margin(tick_margin),
                )
            } else {
                el.child(
                    LineChart::new(pts)
                        .x(|d| d.x.clone())
                        .y(|d| d.y)
                        .stroke(color)
                        .linear()
                        .tick_margin(tick_margin),
                )
            }
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_pts_uses_utc_clock_labels() {
        let pts = to_pts(&[SeriesPoint {
            t_ms: 3_600_000,
            v: 1.5,
        }]);
        assert_eq!(pts[0].x, "01:00");
        assert_eq!(pts[0].y, 1.5);
    }
}
