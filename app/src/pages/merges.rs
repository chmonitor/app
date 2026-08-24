//! In-flight merges and mutations.

use chm_core::MergeRow;

use gpui::{Context, Render, Window, prelude::*};

use crate::pages::status;
use crate::widgets::{CellVal, Column, data_table};

pub struct MergesPage {
    data: Option<Vec<MergeRow>>,
    error: Option<String>,
}

impl Default for MergesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl MergesPage {
    pub fn new() -> Self {
        Self {
            data: None,
            error: None,
        }
    }

    pub fn set(&mut self, data: Result<Vec<MergeRow>, String>, cx: &mut Context<Self>) {
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

impl Render for MergesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(err) = &self.error {
            return status(format!("merges unavailable: {err}"), cx).into_any_element();
        }
        let Some(rows) = &self.data else {
            return status("loading merges…", cx).into_any_element();
        };
        if rows.is_empty() {
            return status("no merges or mutations in flight", cx).into_any_element();
        }
        let columns = vec![
            Column {
                name: "database".into(),
                width: Some(110.0),
            },
            Column {
                name: "table".into(),
                width: Some(140.0),
            },
            Column {
                name: "type".into(),
                width: Some(80.0),
            },
            Column {
                name: "progress".into(),
                width: Some(80.0),
            },
            Column {
                name: "parts".into(),
                width: Some(72.0),
            },
            Column {
                name: "memory".into(),
                width: Some(88.0),
            },
            Column {
                name: "elapsed".into(),
                width: None,
            },
        ];
        let body = rows
            .iter()
            .map(|r| {
                vec![
                    CellVal::Text(r.database.clone()),
                    CellVal::Text(r.table.clone()),
                    CellVal::Text(if r.is_mutation {
                        "mutation".into()
                    } else {
                        "merge".into()
                    }),
                    CellVal::Text(format!("{:.0}%", (r.progress * 100.0).clamp(0.0, 100.0))),
                    CellVal::Num(r.num_parts as f64),
                    CellVal::Bytes(r.total_memory_bytes),
                    CellVal::DurMs(r.elapsed_sec * 1000.0),
                ]
            })
            .collect();
        data_table(columns, body).into_any_element()
    }
}
