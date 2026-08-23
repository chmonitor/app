//! Time-series line chart: axes, tick labels and one polyline per series,
//! painted on a gpui [`canvas`](bezel::gpui::canvas). The math lives in
//! [`geometry`](super::geometry); this file only turns pixels into paint.

use super::geometry::{Bounds, format_count, nice_scale, points_to_px};
use bezel::gpui::{
    App, Bounds as GBounds, Font, FontFeatures, FontWeight, Hsla, IntoElement, PathBuilder, Pixels,
    TextAlign, TextRun, Window, canvas, div, fill, font, point, prelude::*, px, size,
};
use bezel::theme::{Theme, current_appearance, hairline};
use chm_core::SeriesPoint;

/// One named line on a chart. `accent` picks the theme's accent color instead
/// of the muted foreground, so a primary series can out-shine its peers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NamedSeries {
    pub name: String,
    pub points: Vec<SeriesPoint>,
    pub accent: bool,
}

/// Layout metrics for one chart, resolved against the actual element size.
struct ChartLayout {
    plot: Bounds,
}

const PAD_LEFT: f64 = 46.0;
const PAD_RIGHT: f64 = 12.0;
const PAD_TOP: f64 = 30.0;
const PAD_BOTTOM: f64 = 22.0;
const MIN_PLOT_W: f64 = 40.0;
const MIN_PLOT_H: f64 = 40.0;
const STROKE_WIDTH: f32 = 1.5;
const AXIS_FONT_SIZE: f32 = 10.0;

fn mono_font(t: &Theme) -> Font {
    let mut f = font(t.font_mono.clone());
    f.features = FontFeatures::default();
    f
}

/// Tick label text: compact counts, except bytes-style units which keep one
/// decimal (`2.5G` vs `1.0 GiB` both read fine on an axis).
fn tick_label(v: f64) -> String {
    format_count(v)
}

/// The full chart element: title, plot area with grid lines, tick labels on
/// both axes, and one polyline per series. Empty series render the empty
/// frame; nothing panics on degenerate data.
pub fn line_chart(title: &str, unit: &str, series: Vec<NamedSeries>) -> impl IntoElement {
    let t = Theme::for_appearance(current_appearance());
    let title = title.to_string();
    let unit = unit.to_string();

    let title_el = div()
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text)
        .child(title);

    let unit_el = div()
        .text_size(px(11.0))
        .text_color(t.text_faint)
        .child(unit);

    let header = div()
        .flex()
        .flex_row()
        .items_baseline()
        .gap(px(8.0))
        .child(title_el)
        .child(unit_el);
    let plot_bg_color = t.surface;
    let grid_color = hairline(0.07);
    let axis_text_color = t.text_muted;
    let line_color_muted = t.text_muted;
    let line_color_accent = t.accent;
    let mono = mono_font(&t);
    let font_size = px(AXIS_FONT_SIZE);

    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(header)
        .child(div().flex_1().min_h_0().w_full().child(canvas(
            move |bounds: GBounds<Pixels>, _window: &mut Window, _cx: &mut App| {
                let w =
                    f64::from(bounds.size.width.as_f32()).max(PAD_LEFT + PAD_RIGHT + MIN_PLOT_W);
                let h =
                    f64::from(bounds.size.height.as_f32()).max(PAD_TOP + PAD_BOTTOM + MIN_PLOT_H);
                ChartLayout {
                    plot: Bounds {
                        x: PAD_LEFT,
                        y: PAD_TOP,
                        w: w - PAD_LEFT - PAD_RIGHT,
                        h: h - PAD_TOP - PAD_BOTTOM,
                    },
                }
            },
            move |bounds: GBounds<Pixels>,
                  layout: ChartLayout,
                  window: &mut Window,
                  _cx: &mut App| {
                let origin = bounds.origin;
                let plot = layout.plot;

                window.paint_quad(fill(
                    GBounds::<Pixels> {
                        origin: origin + point(px(plot.x as f32), px(plot.y as f32)),
                        size: size(px(plot.w as f32), px(plot.h as f32)),
                    },
                    plot_bg_color,
                ));

                let all_values: Vec<f64> = series
                    .iter()
                    .flat_map(|s| s.points.iter().map(|p| p.v))
                    .collect();
                let (data_min, data_max) = if all_values.is_empty() {
                    (0.0, 1.0)
                } else {
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for v in &all_values {
                        if v.is_finite() {
                            lo = lo.min(*v);
                            hi = hi.max(*v);
                        }
                    }
                    if !lo.is_finite() || !hi.is_finite() {
                        (0.0, 1.0)
                    } else {
                        (lo, hi)
                    }
                };

                let (y_min, y_max, y_ticks) = nice_scale(data_min, data_max, 4);
                let y_span = if y_max > y_min { y_max - y_min } else { 1.0 };

                for tick in &y_ticks {
                    let frac = (tick - y_min) / y_span;
                    let y = plot.y + plot.h - frac * plot.h;
                    if y < plot.y - 0.5 || y > plot.y + plot.h + 0.5 {
                        continue;
                    }
                    let y = y.clamp(plot.y, plot.y + plot.h);
                    window.paint_quad(fill(
                        GBounds::<Pixels> {
                            origin: origin + point(px(plot.x as f32), px(y as f32 - 0.5)),
                            size: size(px(plot.w as f32), px(1.0)),
                        },
                        grid_color,
                    ));
                    let label = tick_label(*tick);
                    let label_len = label.len();
                    let shaped = window
                        .text_system()
                        .shape_text(
                            label.into(),
                            font_size,
                            &[TextRun {
                                len: label_len,
                                font: mono.clone(),
                                color: axis_text_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                            None,
                        )
                        .ok();
                    if let Some(mut lines) = shaped
                        && let Some(line) = lines.first_mut()
                    {
                        let line_w = line.unwrapped_layout.width;
                        let lx = origin.x + px(plot.x as f32) - line_w - px(6.0);
                        let ly = origin.y + px(y as f32) - px(AXIS_FONT_SIZE * 0.62);
                        let _ = line.paint(
                            point(lx, ly),
                            px(AXIS_FONT_SIZE * 1.25),
                            TextAlign::Right,
                            None,
                            window,
                            _cx,
                        );
                    }
                }

                let x_ticks = super::geometry::x_time_ticks(
                    series.first().map(|s| s.points.as_slice()).unwrap_or(&[]),
                    plot.w,
                    5,
                );
                for (t_ms, frac) in x_ticks {
                    if !(0.0..=1.0).contains(&frac) {
                        continue;
                    }
                    let x = plot.x + frac * plot.w;
                    let label = super::geometry::format_time_of_day(t_ms);
                    let shaped = window
                        .text_system()
                        .shape_text(
                            label.clone().into(),
                            font_size,
                            &[TextRun {
                                len: label.len(),
                                font: mono.clone(),
                                color: axis_text_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                            None,
                        )
                        .ok();
                    if let Some(mut lines) = shaped
                        && let Some(line) = lines.first_mut()
                    {
                        let lx = origin.x + px(x as f32) - line.unwrapped_layout.width / 2.0;
                        let ly = origin.y + px((plot.y + plot.h) as f32) + px(5.0);
                        let _ = line.paint(
                            point(lx, ly),
                            px(AXIS_FONT_SIZE * 1.25),
                            TextAlign::Left,
                            None,
                            window,
                            _cx,
                        );
                    }
                }

                let px_points: Vec<Vec<(f64, f64)>> = series
                    .iter()
                    .map(|s| points_to_px(&s.points, plot, y_min, y_max))
                    .collect();
                for (s, pts) in series.iter().zip(px_points) {
                    if pts.len() < 2 {
                        continue;
                    }
                    let color: Hsla = if s.accent {
                        line_color_accent
                    } else {
                        line_color_muted
                    };
                    let mut builder = PathBuilder::stroke(px(STROKE_WIDTH));
                    let first = pts[0];
                    builder.move_to(point(
                        origin.x + px(first.0 as f32),
                        origin.y + px(first.1 as f32),
                    ));
                    for p in &pts[1..] {
                        builder
                            .line_to(point(origin.x + px(p.0 as f32), origin.y + px(p.1 as f32)));
                    }
                    if let Ok(stroke_path) = builder.build() {
                        window.paint_path(stroke_path, color);
                    }
                }
            },
        )))
}

/// Baseline helper so callers can sanity-check a series against the axis the
/// chart would choose, without painting.
pub fn chart_axis_for(series: &[NamedSeries]) -> (f64, f64, Vec<f64>) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in series {
        for p in &s.points {
            if p.v.is_finite() {
                lo = lo.min(p.v);
                hi = hi.max(p.v);
            }
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return nice_scale(0.0, 1.0, 4);
    }
    nice_scale(lo, hi, 4)
}
