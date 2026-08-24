//! Data table: header row, body rows, right-aligned numerics.

use gpui::{div, prelude::*, px};
use gpui_component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};

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
pub fn data_table(columns: Vec<Column>, rows: Vec<Vec<CellVal>>) -> impl IntoElement {
    let header = TableHeader::new().child(TableRow::new().children(columns.iter().map(|col| {
        let mut head = TableHead::new().child(col.name.clone());
        if let Some(w) = col.width {
            head = head.w(px(w));
        }
        head
    })));

    let body = TableBody::new().children(rows.into_iter().map(|cells| {
        let cols = &columns;
        TableRow::new().children((0..cols.len()).map(|i| {
            let mut cell = match cells.get(i) {
                Some(value) => {
                    let mut c = TableCell::new().child(value.display());
                    if value.numeric() {
                        c = c.text_right();
                    }
                    c
                }
                None => TableCell::new().child(div()),
            };
            if let Some(w) = cols.get(i).and_then(|c| c.width) {
                cell = cell.w(px(w));
            }
            cell
        }))
    }));

    Table::new().child(header).child(body)
}
