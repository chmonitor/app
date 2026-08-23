//! Data table: header row, zebra-striped body rows, monospace right-aligned
//! numerics. Cell values render through the [`geometry`](super::geometry)
//! formatters so units stay consistent across the app.

use super::geometry::{format_bytes, format_count, format_duration_ms};
use bezel::gpui::{Div, Hsla, div, prelude::*, px};
use bezel::theme::{Theme, current_appearance, hairline};

/// One cell's value; the variant picks the formatter and the alignment.
#[derive(Debug, Clone, PartialEq)]
pub enum CellVal {
    /// Left-aligned prose.
    Text(String),
    /// Right-aligned compact count (`format_count`).
    Num(f64),
    /// Right-aligned binary size (`format_bytes`).
    Bytes(u64),
    /// Right-aligned duration (`format_duration_ms`).
    DurMs(f64),
}

/// A column: header label plus an optional fixed width in logical pixels.
/// `None` lets the column flex with the container.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Column {
    pub name: String,
    pub width: Option<f32>,
}

/// Render one cell to a styled div. Public within the crate so pages can
/// reuse the exact cell styling inside custom rows.
impl CellVal {
    pub fn render(self) -> Div {
        let t = Theme::for_appearance(current_appearance());
        match self {
            CellVal::Text(text) => div()
                .text_size(px(11.0))
                .text_color(t.text)
                .truncate()
                .child(text),
            CellVal::Num(n) => numeric_cell(format_count(n), t.text),
            CellVal::Bytes(b) => numeric_cell(format_bytes(b), t.text),
            CellVal::DurMs(ms) => numeric_cell(format_duration_ms(ms), t.text),
        }
    }
}

fn numeric_cell(text: String, color: Hsla) -> Div {
    let t = Theme::for_appearance(current_appearance());
    div()
        .font_family(t.font_mono.clone())
        .text_size(px(11.0))
        .text_color(color)
        .flex()
        .justify_end()
        .child(text)
}

/// Header + striped rows. Rows may be ragged: short rows simply leave the
/// trailing columns empty.
pub fn data_table(columns: Vec<Column>, rows: Vec<Vec<CellVal>>) -> impl IntoElement {
    let t = Theme::for_appearance(current_appearance());
    let border = t.border;
    let stripe = hairline(0.03);
    let muted = t.text_muted;

    let mut header_cells = Vec::with_capacity(columns.len());
    for col in &columns {
        let mut cell = div().child(
            div()
                .text_size(px(10.0))
                .text_color(muted)
                .truncate()
                .child(col.name.clone()),
        );
        match col.width {
            Some(w) => cell = cell.w(px(w)).flex_none(),
            None => cell = cell.flex_1(),
        }
        header_cells.push(cell);
    }

    let mut row_els = Vec::with_capacity(rows.len());
    for (ix, cells) in rows.iter().enumerate() {
        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(6.0));
        if ix % 2 == 1 {
            row = row.bg(stripe);
        }
        for (col_ix, value) in cells.iter().enumerate() {
            let align_right = !matches!(value, CellVal::Text(_));
            let mut cell = value.clone().render();
            if align_right
                && let Some(col) = columns.get(col_ix)
                && col.width.is_none()
            {
                cell = cell.flex_1();
            } else if columns.get(col_ix).is_some_and(|c| c.width.is_some()) {
                cell = cell.flex_none();
                if let Some(w) = columns.get(col_ix).and_then(|c| c.width) {
                    cell = cell.w(px(w));
                }
            } else {
                cell = cell.flex_1();
            }
            row = row.child(cell);
        }
        for _ in cells.len()..columns.len() {
            row = row.child(div().flex_1());
        }
        row_els.push(row);
    }

    div()
        .w_full()
        .overflow_hidden()
        .border_1()
        .border_color(border)
        .rounded(px(Theme::PANEL_RADIUS))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(12.0))
                .py(px(8.0))
                .bg(hairline(0.04))
                .border_b_1()
                .border_color(border)
                .children(header_cells),
        )
        .children(row_els)
}
