//! Table stats.

use chm_core::TableStat;

use gpui::{Context, Render, Window, prelude::*};

use crate::pages::status;
use crate::widgets::{CellVal, Column, data_table};

pub struct TablesPage {
    data: Option<Vec<TableStat>>,
    error: Option<String>,
}

impl Default for TablesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl TablesPage {
    pub fn new() -> Self {
        Self {
            data: None,
            error: None,
        }
    }

    pub fn set(&mut self, data: Result<Vec<TableStat>, String>, cx: &mut Context<Self>) {
        match data {
            Ok(v) => {
                self.data = Some(v);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }
}

impl Render for TablesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(err) = &self.error {
            return status(format!("tables unavailable: {err}"), cx).into_any_element();
        }
        let Some(rows) = &self.data else {
            return status("loading tables…", cx).into_any_element();
        };
        if rows.is_empty() {
            return status("no tables", cx).into_any_element();
        }
        let columns = vec![
            Column {
                name: "database".into(),
                width: Some(110.0),
            },
            Column {
                name: "table".into(),
                width: Some(160.0),
            },
            Column {
                name: "engine".into(),
                width: Some(160.0),
            },
            Column {
                name: "parts".into(),
                width: Some(72.0),
            },
            Column {
                name: "rows".into(),
                width: Some(88.0),
            },
            Column {
                name: "size".into(),
                width: Some(88.0),
            },
            Column {
                name: "ratio".into(),
                width: Some(64.0),
            },
            Column {
                name: "modified".into(),
                width: None,
            },
        ];
        let body = rows
            .iter()
            .map(|r| {
                let modified = r
                    .last_modified
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "—".into());
                vec![
                    CellVal::Text(r.database.clone()),
                    CellVal::Text(r.name.clone()),
                    CellVal::Text(r.engine.clone()),
                    CellVal::Num(r.parts as f64),
                    CellVal::Num(r.rows as f64),
                    CellVal::Bytes(r.bytes_on_disk),
                    CellVal::Text(format!("{:.1}×", r.compressed_ratio)),
                    CellVal::Text(modified),
                ]
            })
            .collect();
        data_table("tables", columns, body, cx).into_any_element()
    }
}
