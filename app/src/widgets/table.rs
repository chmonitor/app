//! Data table: header row, body rows, right-aligned numerics.
//! Semantic structure comes from gpui-base; colors and density from the theme.

use gpui::{App, ElementId, div, prelude::*, px};
use gpui_base::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_component::ActiveTheme as _;

use super::geometry::{format_bytes, format_count, format_duration_ms};

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

impl CellVal {
    fn display(&self) -> String {
        match self {
            CellVal::Text(text) => text.clone(),
            CellVal::Num(n) => format_count(*n),
            CellVal::Bytes(b) => format_bytes(*b),
            CellVal::DurMs(ms) => format_duration_ms(*ms),
        }
    }

    fn numeric(&self) -> bool {
        !matches!(self, CellVal::Text(_))
    }
}

/// A column: header label plus an optional fixed width in logical pixels.
/// `None` lets the column flex with the container.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Column {
    pub name: String,
    pub width: Option<f32>,
}

/// Header + rows. Rows may be ragged: short rows simply leave the
/// trailing columns empty.
pub fn data_table(
    id: impl Into<ElementId>,
    columns: Vec<Column>,
    rows: Vec<Vec<CellVal>>,
    cx: &App,
) -> impl IntoElement {
    let id = id.into();
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    let header_bg = cx.theme().secondary;
    let n_cols = columns.len();
    let n_rows = rows.len();

    let header = TableHeader::new("header").child(TableRow::new("header-row", 1).flex().children(
        columns.iter().enumerate().map(|(i, col)| {
            let mut head = TableHead::new(("head", i), i + 1)
                .px_3()
                .py_2()
                .text_xs()
                .text_color(muted)
                .child(col.name.clone());
            if let Some(w) = col.width {
                head = head.w(px(w)).flex_none();
            } else {
                head = head.flex_1();
            }
            head
        }),
    ));

    let body =
        TableBody::new("body").children(rows.into_iter().enumerate().map(|(row_ix, cells)| {
            let cols = &columns;
            TableRow::new(("row", row_ix), row_ix + 2)
                .flex()
                .border_t_1()
                .border_color(border)
                .children((0..cols.len()).map(|i| {
                    let mut cell = match cells.get(i) {
                        Some(value) => {
                            let mut c = TableCell::new(format!("cell-{row_ix}-{i}"), i + 1)
                                .px_3()
                                .py_1()
                                .text_xs()
                                .child(value.display());
                            if value.numeric() {
                                c = c.text_right();
                            }
                            c
                        }
                        None => TableCell::new(format!("empty-{row_ix}-{i}"), i + 1).child(div()),
                    };
                    if let Some(w) = cols.get(i).and_then(|c| c.width) {
                        cell = cell.w(px(w)).flex_none();
                    } else {
                        cell = cell.flex_1();
                    }
                    cell
                }))
        }));

    Table::new(id)
        .w_full()
        .overflow_hidden()
        .border_1()
        .border_color(border)
        .rounded(cx.theme().radius)
        .row_count(n_rows + 1)
        .column_count(n_cols)
        .child(header.bg(header_bg))
        .child(body)
}
