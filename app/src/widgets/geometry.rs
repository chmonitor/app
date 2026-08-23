//! Pure layout math and human formatting for widgets. No gpui types here:
//! everything is `f64`/`i64` so it unit-tests without a window or an app.

use chm_core::SeriesPoint;

/// Axis-aligned rectangle in pixels (or any uniform space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// "Nice" axis rounding: pick the largest 1/2/2.5/5 × 10ⁿ step that fits the
/// target tick count, snap `[min, max]` outward to step multiples, and anchor
/// non-negative data at zero so bars and lines read from a true baseline.
///
/// Degenerate inputs are handled: a flat series widens to a sane span around
/// zero, non-finite values fall back to `[0, 1]`, and inverted inputs are
/// swapped.
pub fn nice_scale(min: f64, max: f64, target_ticks: usize) -> (f64, f64, Vec<f64>) {
    let target_ticks = target_ticks.max(1);

    let (lo, hi) = if !min.is_finite() || !max.is_finite() {
        (0.0, 1.0)
    } else if min > max {
        (max, min)
    } else {
        (min, max)
    };

    let mut span = hi - lo;
    if !span.is_finite() || span <= 0.0 {
        let center = if lo.is_finite() { lo } else { 0.0 };
        let pad = if center == 0.0 {
            1.0
        } else {
            center.abs().max(1.0) * 0.05
        };
        span = 2.0 * pad;
    }

    let raw_step = span / target_ticks as f64;
    let mag = 10f64.powf(raw_step.abs().log10().floor());
    let norm = (raw_step / mag).max(1.0);
    let step = mag
        * if norm < 2.0 {
            1.0
        } else if norm < 2.5 {
            2.0
        } else if norm < 5.0 {
            2.5
        } else {
            5.0
        };

    let axis_min = if lo >= 0.0 {
        0.0
    } else {
        (lo / step).floor() * step
    };
    let mut axis_max = (hi / step).ceil() * step;
    if axis_max <= axis_min {
        axis_max = axis_min + step;
    }

    let count = (((axis_max - axis_min) / step).round() as usize).max(1);
    let mut ticks = Vec::with_capacity(count + 1);
    for i in 0..=count {
        ticks.push(axis_min + step * i as f64);
    }
    (axis_min, axis_max, ticks)
}

/// Map series points into pixel space inside `bounds`.
///
/// X positions come from each point's time normalized over the full
/// `[t_first, t_last]` span of the input (not per-index spacing), so gaps in
/// time show as gaps in pixels; Y comes from the supplied fixed axis range.
/// Empty input returns an empty vector; a single point lands centered
/// horizontally at its value's Y.
pub fn points_to_px(
    points: &[SeriesPoint],
    bounds: Bounds,
    y_min: f64,
    y_max: f64,
) -> Vec<(f64, f64)> {
    if points.is_empty() {
        return Vec::new();
    }
    let t_min = points.iter().map(|p| p.t_ms).min().unwrap_or(0);
    let t_max = points.iter().map(|p| p.t_ms).max().unwrap_or(0);
    let t_span = (t_max - t_min).max(0) as f64;

    let y_span = if y_max > y_min { y_max - y_min } else { 1.0 };

    points
        .iter()
        .map(|p| {
            let x = bounds.x
                + if t_span > 0.0 {
                    (p.t_ms - t_min) as f64 / t_span * bounds.w
                } else {
                    bounds.w / 2.0
                };
            let y = bounds.y + bounds.h - ((p.v - y_min) / y_span * bounds.h);
            (x, y)
        })
        .collect()
}

/// Binary (IEC) units with one decimal above 100: `"1.5 GiB"`, `"893 MiB"`.
pub fn format_bytes(b: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = b as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{b} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Human durations from milliseconds: sub-second precision below 1 s, seconds
/// with one decimal below a minute, `m:ss` up to an hour, `Xh YYm` beyond.
pub fn format_duration_ms(ms: f64) -> String {
    if !ms.is_finite() {
        return "—".to_string();
    }
    let ms = ms.max(0.0);
    if ms < 1.0 {
        return format!("{:.0} µs", ms * 1000.0);
    }
    if ms < 1000.0 {
        return format!("{ms:.0} ms");
    }
    let secs = ms / 1000.0;
    if secs < 60.0 {
        // Round half-up so 12.45 reads as "12.5 s" and never "12.4 s".
        return format!("{:.1}", (secs * 10.0).round() / 10.0) + " s";
    }
    let total = secs.round() as u64;
    let mins = total / 60;
    let rem_secs = total % 60;
    if mins < 60 {
        format!("{mins}:{rem_secs:02}")
    } else {
        let hours = mins / 60;
        let rem_mins = mins % 60;
        format!("{hours}h {rem_mins:02}m")
    }
}

/// Compact counts with SI suffixes: `999`, `"1.2k"`, `"3.4M"`.
pub fn format_count(n: f64) -> String {
    if !n.is_finite() {
        return "—".to_string();
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let a = n.abs();
    if a < 1000.0 {
        return format!("{sign}{}", a.trunc());
    }
    const UNITS: [(&str, f64); 4] = [("k", 1e3), ("M", 1e6), ("G", 1e9), ("T", 1e12)];
    let mut chosen = UNITS[0];
    for candidate in UNITS {
        if a >= candidate.1 {
            chosen = candidate;
        }
    }
    let scaled = a / chosen.1;
    // Always one decimal past the comma: "1.0k", "123.5M", "1000.0k".
    format!("{sign}{scaled:.1}{}", chosen.0)
}

/// Evenly spaced x-axis ticks over a series' own time span: `(t_ms,
/// fraction)` pairs where `fraction ∈ [0, 1]` positions the tick across the
/// plot width. Empty or flat input yields a single tick at the left edge.
pub fn x_time_ticks(points: &[SeriesPoint], plot_w: f64, target_ticks: usize) -> Vec<(i64, f64)> {
    let _ = plot_w;
    if points.is_empty() {
        return Vec::new();
    }
    let target_ticks = target_ticks.max(1);
    let t_min = points[0].t_ms;
    let t_max = points[points.len() - 1].t_ms;
    if t_max <= t_min {
        return vec![(t_min, 0.0)];
    }
    let span = t_max - t_min;
    let step = (span / target_ticks as i64).max(1);
    let first = (t_min + step - 1) / step * step;
    let mut out = Vec::new();
    let mut t = first;
    while t <= t_max {
        out.push((t, (t - t_min) as f64 / span as f64));
        t += step;
    }
    out
}

/// `HH:MM` wall-clock label for an epoch-milliseconds timestamp (UTC).
pub fn format_time_of_day(t_ms: i64) -> String {
    let secs = t_ms.div_euclid(1000);
    let day_secs = secs.rem_euclid(86_400);
    format!("{:02}:{:02}", day_secs / 3600, (day_secs % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(t_ms: i64, v: f64) -> SeriesPoint {
        SeriesPoint { t_ms, v }
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    // ---- nice_scale ----

    #[test]
    fn nice_scale_basic_decade() {
        let (lo, hi, ticks) = nice_scale(3.0, 97.0, 5);
        assert!(approx(lo, 0.0), "{lo}");
        assert!(approx(hi, 100.0), "{hi}");
        assert_eq!(ticks.first().copied(), Some(lo));
        assert_eq!(ticks.last().copied(), Some(hi));
        assert_eq!(ticks.len(), 11);
        for w in ticks.windows(2) {
            assert!(w[1] > w[0], "ticks must ascend: {ticks:?}");
        }
    }

    #[test]
    fn nice_scale_flat_series_widens() {
        let (lo, hi, ticks) = nice_scale(7.0, 7.0, 5);
        assert!(lo < hi, "flat input must still produce a range");
        assert!(lo <= 7.0 && hi >= 7.0, "range must contain the value");
        assert!(!ticks.is_empty());
        assert!(approx(*ticks.first().unwrap(), lo));
        assert!(approx(*ticks.last().unwrap(), hi));
    }

    #[test]
    fn nice_scale_zero_flat() {
        let (lo, hi, ticks) = nice_scale(0.0, 0.0, 4);
        assert!(lo < hi);
        assert!(lo <= 0.0 && hi >= 0.0);
        assert!(!ticks.is_empty());
    }

    #[test]
    fn nice_scale_inverted_inputs_swapped() {
        let (a_lo, a_hi, _) = nice_scale(0.0, 50.0, 5);
        let (b_lo, b_hi, _) = nice_scale(50.0, 0.0, 5);
        assert!(approx(a_lo, b_lo) && approx(a_hi, b_hi));
    }

    #[test]
    fn nice_scale_non_finite_falls_back() {
        let (lo, hi, ticks) = nice_scale(f64::NAN, f64::INFINITY, 4);
        assert!(lo.is_finite() && hi.is_finite() && hi > lo);
        assert!(!ticks.is_empty());
        assert!(ticks.iter().all(|t| t.is_finite()));
    }

    #[test]
    fn nice_scale_tiny_values_use_fractional_steps() {
        let (lo, hi, ticks) = nice_scale(1e-9, 2.5e-9, 4);
        assert!(approx(lo, 0.0), "{lo}");
        assert!(hi >= 2.5e-9, "{hi}");
        assert!(ticks.len() >= 2);
        assert!(ticks.iter().all(|t| t.is_finite()));
    }

    #[test]
    fn nice_scale_huge_values_keep_precision() {
        let (lo, hi, ticks) = nice_scale(1.8e12, 9.2e12, 5);
        assert!(approx(lo, 0.0), "{lo}");
        assert!(approx(hi, 1e13), "{hi}");
        assert!(ticks.len() >= 2);
        for w in ticks.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[test]
    fn nice_scale_negative_range() {
        let (lo, hi, ticks) = nice_scale(-87.0, -13.0, 5);
        assert!(approx(lo, -90.0), "{lo}");
        assert!(approx(hi, -10.0), "{hi}");
        assert_eq!(ticks.first().copied(), Some(lo));
        assert_eq!(ticks.last().copied(), Some(hi));
    }

    #[test]
    fn nice_scale_bracketing_range_snaps_to_itself() {
        let (lo, hi, ticks) = nice_scale(0.0, 10.0, 5);
        assert!(approx(lo, 0.0) && approx(hi, 10.0));
        assert_eq!(ticks.len(), 6);
    }

    #[test]
    fn nice_scale_target_ticks_is_a_floor_not_exact() {
        let (_, _, ticks) = nice_scale(0.0, 1000.0, 3);
        assert!((3..=15).contains(&ticks.len()), "{}", ticks.len());
    }

    #[test]
    fn nice_scale_ticks_are_evenly_spaced() {
        let (_, _, ticks) = nice_scale(-25.0, 125.0, 6);
        let step = ticks[1] - ticks[0];
        for w in ticks.windows(2) {
            assert!(approx(w[1] - w[0], step), "uneven steps: {ticks:?}");
        }
        assert!(step > 0.0);
    }

    // ---- points_to_px ----

    const CHART: Bounds = Bounds {
        x: 40.0,
        y: 10.0,
        w: 360.0,
        h: 200.0,
    };

    #[test]
    fn points_to_px_maps_time_and_value_linearly() {
        let pts = vec![point(0, 0.0), point(1000, 10.0)];
        let px = points_to_px(&pts, CHART, 0.0, 10.0);
        assert_eq!(px.len(), 2);
        let (x0, y0) = px[0];
        let (x1, y1) = px[1];
        assert!(approx(x0, 40.0), "first point at left edge: {x0}");
        assert!(approx(x1, 400.0), "last point at right edge: {x1}");
        assert!(
            approx(y0, 210.0),
            "zero value sits on the bottom edge: {y0}"
        );
        assert!(approx(y1, 10.0), "max value sits on the top edge: {y1}");
    }

    #[test]
    fn points_to_px_x_is_normalized_by_time_span_not_index() {
        let even = points_to_px(
            &[point(0, 1.0), point(500, 1.0), point(1000, 1.0)],
            CHART,
            0.0,
            2.0,
        );
        let gapped = points_to_px(
            &[point(0, 1.0), point(900, 1.0), point(1000, 1.0)],
            CHART,
            0.0,
            2.0,
        );
        assert!(approx(
            gapped[1].0 - gapped[0].0,
            (even[1].0 - even[0].0) * 1.8
        ));
        assert!(approx(
            gapped[2].0 - gapped[1].0,
            (even[2].0 - even[1].0) * 0.2
        ));
    }

    #[test]
    fn points_to_px_empty_input() {
        assert!(points_to_px(&[], CHART, 0.0, 1.0).is_empty());
    }

    #[test]
    fn points_to_px_single_point_centered() {
        let px = points_to_px(&[point(42, 5.0)], CHART, 0.0, 10.0);
        assert_eq!(px.len(), 1);
        assert!(approx(px[0].0, CHART.x + CHART.w / 2.0), "{:?}", px[0]);
        assert!(approx(px[0].1, CHART.y + CHART.h / 2.0), "{:?}", px[0]);
    }

    #[test]
    fn points_to_px_degenerate_y_axis_does_not_divide_by_zero() {
        let px = points_to_px(&[point(0, 3.0), point(10, 7.0)], CHART, 5.0, 5.0);
        assert_eq!(px.len(), 2);
        assert!(px.iter().all(|(x, y)| x.is_finite() && y.is_finite()));
    }

    #[test]
    fn points_to_px_unsorted_input_keeps_input_order() {
        let pts = vec![point(2000, 1.0), point(0, 1.0), point(1000, 1.0)];
        let px = points_to_px(&pts, CHART, 0.0, 2.0);
        assert_eq!(px.len(), 3);
        assert!(approx(px[0].0, 400.0), "input order preserved: {:?}", px);
        assert!(approx(px[1].0, 40.0));
        assert!(approx(px[2].0, 220.0));
    }

    #[test]
    fn points_to_px_clamps_only_against_axis_extremes() {
        let pts = vec![point(0, -5.0), point(1000, 20.0)];
        let px = points_to_px(&pts, CHART, 0.0, 10.0);
        // Values outside the axis overshoot linearly: -5 is half a span below
        // the bottom (y = 210 + 100), 20 is a full span above the top.
        assert!(
            approx(px[0].1, 310.0) && approx(px[1].1, -190.0),
            "values outside the axis overshoot linearly: {:?}",
            px
        );
    }

    #[test]
    fn points_to_px_negative_times() {
        let pts = vec![point(-2000, 0.0), point(2000, 4.0)];
        let px = points_to_px(&pts, CHART, 0.0, 4.0);
        assert!(approx(px[0].0, 40.0) && approx(px[1].0, 400.0));
        assert!(approx(px[0].1, 210.0) && approx(px[1].1, 10.0));
    }

    // ---- format_bytes ----

    #[test]
    fn bytes_zero() {
        assert_eq!(format_bytes(0), "0 B");
    }

    #[test]
    fn bytes_under_one_kib_stay_raw() {
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn bytes_kib_boundary() {
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }

    #[test]
    fn bytes_mib() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(1536 * 1024), "1.5 MiB");
    }

    #[test]
    fn bytes_gib() {
        let gib = 1024_u64.pow(3);
        assert_eq!(format_bytes(gib), "1.0 GiB");
        assert_eq!(format_bytes(gib * 3 / 2), "1.5 GiB");
    }

    #[test]
    fn bytes_tib() {
        let tib = 1024_u64.pow(4);
        assert_eq!(format_bytes(tib), "1.0 TiB");
        assert_eq!(format_bytes(tib * 2), "2.0 TiB");
    }

    #[test]
    fn bytes_large_whole_values_drop_the_decimal() {
        assert_eq!(format_bytes(893 * 1024 * 1024), "893 MiB");
        assert_eq!(format_bytes(120 * 1024), "120 KiB");
    }

    #[test]
    fn bytes_huge_saturates_at_top_unit() {
        let big = u64::MAX;
        let s = format_bytes(big);
        assert!(s.ends_with(" EiB"), "{s}");
    }

    // ---- format_duration_ms ----

    #[test]
    fn duration_sub_millisecond() {
        assert_eq!(format_duration_ms(0.0), "0 µs");
        assert_eq!(format_duration_ms(0.5), "500 µs");
    }

    #[test]
    fn duration_milliseconds() {
        assert_eq!(format_duration_ms(1.0), "1 ms");
        assert_eq!(format_duration_ms(450.0), "450 ms");
        // 999.9 ms rounds to seconds as "1.0 s"; under the 1 s cutoff it is
        // still milliseconds, rounded to whole ms.
        assert_eq!(format_duration_ms(999.9), "1000 ms");
    }

    #[test]
    fn duration_seconds_one_decimal() {
        assert_eq!(format_duration_ms(1000.0), "1.0 s");
        assert_eq!(format_duration_ms(12450.0), "12.5 s"); // 12.45 rounds to 12.5
        assert_eq!(format_duration_ms(59_900.0), "59.9 s");
    }

    #[test]
    fn duration_minutes() {
        assert_eq!(format_duration_ms(60_000.0), "1:00");
        assert_eq!(format_duration_ms(62_000.0), "1:02");
        // 59:59.83 truncates at second precision; the hour rollover happens
        // at exactly 3_600_000 ms ("1h 00m").
        assert_eq!(format_duration_ms(3_599_000.0), "59:59");
    }

    #[test]
    fn duration_hours() {
        assert_eq!(format_duration_ms(3_600_000.0), "1h 00m");
        assert_eq!(format_duration_ms(3_720_000.0), "1h 02m");
        assert_eq!(format_duration_ms(86_400_000.0), "24h 00m");
    }

    #[test]
    fn duration_negative_and_non_finite_are_safe() {
        assert_eq!(format_duration_ms(-5.0), "0 µs");
        assert_eq!(format_duration_ms(f64::NAN), "—");
        assert_eq!(format_duration_ms(f64::INFINITY), "—");
    }

    // ---- format_count ----

    #[test]
    fn count_small_integers_plain() {
        assert_eq!(format_count(0.0), "0");
        assert_eq!(format_count(7.0), "7");
        assert_eq!(format_count(999.0), "999");
    }

    #[test]
    fn count_thousands() {
        assert_eq!(format_count(1000.0), "1.0k");
        assert_eq!(format_count(1234.0), "1.2k");
        assert_eq!(format_count(999_999.0), "1000.0k");
    }

    #[test]
    fn count_millions() {
        assert_eq!(format_count(3_400_000.0), "3.4M");
        assert_eq!(format_count(123_456_789.0), "123.5M");
    }

    #[test]
    fn count_billions_and_trillions() {
        assert_eq!(format_count(2.5e9), "2.5G");
        assert_eq!(format_count(1.0e12), "1.0T");
    }

    #[test]
    fn count_negatives_keep_sign() {
        assert_eq!(format_count(-1234.0), "-1.2k");
        assert_eq!(format_count(-3_400_000.0), "-3.4M");
        assert_eq!(format_count(-42.0), "-42");
    }

    #[test]
    fn count_non_finite_placeholder() {
        assert_eq!(format_count(f64::NAN), "—");
        assert_eq!(format_count(f64::NEG_INFINITY), "—");
    }

    #[test]
    fn count_fractional_parts_truncated_for_display() {
        assert_eq!(format_count(1234.9), "1.2k");
    }

    // ---- x_time_ticks ----

    #[test]
    fn time_ticks_empty_series() {
        assert!(x_time_ticks(&[], 360.0, 5).is_empty());
    }

    #[test]
    fn time_ticks_flat_span_single_left_tick() {
        let ticks = x_time_ticks(&[point(1000, 1.0)], 360.0, 5);
        assert_eq!(ticks, vec![(1000, 0.0)]);
    }

    #[test]
    fn time_ticks_are_aligned_and_in_range() {
        let pts: Vec<SeriesPoint> = (0..=60)
            .map(|i| point(1_700_000_000_000 + i * 60_000, i as f64))
            .collect();
        let ticks = x_time_ticks(&pts, 360.0, 5);
        assert!(!ticks.is_empty());
        let span = pts[pts.len() - 1].t_ms - pts[0].t_ms;
        let step_ms = if ticks.len() > 1 {
            ticks[1].0 - ticks[0].0
        } else {
            span
        };
        for (t, frac) in &ticks {
            assert!(
                *frac >= 0.0 && *frac <= 1.0,
                "fraction out of range: {frac}"
            );
            assert_eq!(t % step_ms, 0, "tick not aligned to absolute time");
        }
        for w in ticks.windows(2) {
            assert!(w[1].0 > w[0].0, "ticks must ascend");
        }
    }

    #[test]
    fn time_ticks_target_count_approximated() {
        let pts: Vec<SeriesPoint> = (0..=100).map(|i| point(i * 10_000, 1.0)).collect();
        let ticks = x_time_ticks(&pts, 500.0, 4);
        assert!((1..=8).contains(&ticks.len()), "{}", ticks.len());
        assert!(approx(ticks[0].1, 0.0));
    }

    #[test]
    fn time_of_day_formats_utc() {
        assert_eq!(format_time_of_day(0), "00:00");
        assert_eq!(format_time_of_day(3_600_000 + 61_000), "01:01");
        // 23h in ms plus a partial minute stays inside the same hour.
        assert_eq!(format_time_of_day(23 * 3_600_000 + 59_999), "23:00");
        assert_eq!(format_time_of_day(86_400_000 + 7_200_000), "02:00");
        assert_eq!(format_time_of_day(-1), "23:59");
    }
}
