//! Replica health table.

use chm_core::ReplicaRow;

use gpui::{Context, Render, Window, prelude::*};

use crate::pages::status;
use crate::widgets::{CellVal, Column, data_table};

pub struct ReplicasPage {
    data: Option<Vec<ReplicaRow>>,
    error: Option<String>,
}

impl Default for ReplicasPage {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplicasPage {
    pub fn new() -> Self {
        Self {
            data: None,
            error: None,
        }
    }

    pub fn set(&mut self, data: Result<Vec<ReplicaRow>, String>, cx: &mut Context<Self>) {
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

impl Render for ReplicasPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(err) = &self.error {
            return status(format!("replicas unavailable: {err}"), cx).into_any_element();
        }
        let Some(rows) = &self.data else {
            return crate::widgets::skeleton::table_block(cx).into_any_element();
        };
        if rows.is_empty() {
            return status("no replicas", cx).into_any_element();
        }
        let columns = vec![
            Column {
                name: "replica".into(),
                width: Some(110.0),
            },
            Column {
                name: "database".into(),
                width: Some(110.0),
            },
            Column {
                name: "table".into(),
                width: Some(140.0),
            },
            Column {
                name: "state".into(),
                width: Some(90.0),
            },
            Column {
                name: "delay".into(),
                width: Some(80.0),
            },
            Column {
                name: "queue".into(),
                width: Some(64.0),
            },
            Column {
                name: "inserts".into(),
                width: Some(72.0),
            },
            Column {
                name: "merges".into(),
                width: Some(72.0),
            },
        ];
        let body = rows
            .iter()
            .map(|r| {
                let state = if r.is_session_expired {
                    "expired"
                } else if r.is_readonly {
                    "readonly"
                } else {
                    "ok"
                };
                vec![
                    CellVal::Text(r.replica_name.clone()),
                    CellVal::Text(r.database.clone()),
                    CellVal::Text(r.table.clone()),
                    CellVal::Text(state.into()),
                    CellVal::Text(if r.absolute_delay_sec < 0.001 {
                        "0s".into()
                    } else {
                        crate::widgets::geometry::format_duration_ms(r.absolute_delay_sec * 1000.0)
                    }),
                    CellVal::Num(r.queue_size as f64),
                    CellVal::Num(r.inserts_in_queue as f64),
                    CellVal::Num(r.merges_in_queue as f64),
                ]
            })
            .collect();
        data_table("replicas", columns, body, cx).into_any_element()
    }
}
