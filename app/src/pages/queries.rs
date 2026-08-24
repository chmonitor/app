//! Running / slow / failed query lists.

use chm_core::QueryRow;

use bezel::gpui::{AnyElement, Context, Render, div, prelude::*, px};

use crate::pages::{heading, status};
use crate::widgets::{CellVal, Column, data_table};

pub struct QueriesPage {
    running: Option<Vec<QueryRow>>,
    slow: Option<Vec<QueryRow>>,
    failed: Option<Vec<QueryRow>>,
    error: Option<String>,
}

impl Default for QueriesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl QueriesPage {
    pub fn new() -> Self {
        Self {
            running: None,
            slow: None,
            failed: None,
            error: None,
        }
    }

    pub fn set(
        &mut self,
        running: Result<Vec<QueryRow>, String>,
        slow: Result<Vec<QueryRow>, String>,
        failed: Result<Vec<QueryRow>, String>,
        cx: &mut Context<Self>,
    ) {
        let mut errors = Vec::new();
        match running {
            Ok(v) => self.running = Some(v),
            Err(e) => errors.push(format!("running: {e}")),
        }
        match slow {
            Ok(v) => self.slow = Some(v),
            Err(e) => errors.push(format!("slow: {e}")),
        }
        match failed {
            Ok(v) => self.failed = Some(v),
            Err(e) => errors.push(format!("failed: {e}")),
        }
        self.error = if errors.is_empty() {
            None
        } else {
            Some(errors.join(" · "))
        };
        cx.notify();
    }
}

fn query_columns(with_exception: bool) -> Vec<Column> {
    let mut cols = vec![
        Column {
            name: "user".into(),
            width: Some(96.0),
        },
        Column {
            name: "elapsed".into(),
            width: Some(80.0),
        },
        Column {
            name: "memory".into(),
            width: Some(80.0),
        },
        Column {
            name: "rows".into(),
            width: Some(72.0),
        },
    ];
    if with_exception {
        cols.push(Column {
            name: "exception".into(),
            width: Some(220.0),
        });
    }
    cols.push(Column {
        name: "query".into(),
        width: None,
    });
    cols
}

fn query_rows(rows: &[QueryRow], with_exception: bool) -> Vec<Vec<CellVal>> {
    rows.iter()
        .map(|r| {
            let mut cells = vec![
                CellVal::Text(r.user.clone()),
                CellVal::DurMs(r.elapsed_ms),
                CellVal::Bytes(r.memory_bytes),
                CellVal::Num(r.read_rows as f64),
            ];
            if with_exception {
                cells.push(CellVal::Text(
                    r.exception.clone().unwrap_or_else(|| "—".into()),
                ));
            }
            cells.push(CellVal::Text(r.normalized_sql.clone()));
            cells
        })
        .collect()
}

fn section(title: &str, rows: Option<&Vec<QueryRow>>, with_exception: bool) -> AnyElement {
    let body: AnyElement = match rows {
        None => status("loading…").into_any_element(),
        Some(rows) if rows.is_empty() => status("none").into_any_element(),
        Some(rows) => data_table(
            query_columns(with_exception),
            query_rows(rows, with_exception),
        )
        .into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(heading(title))
        .child(body)
        .into_any_element()
}

impl Render for QueriesPage {
    fn render(
        &mut self,
        _window: &mut bezel::gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl bezel::gpui::IntoElement {
        if self.running.is_none() && self.slow.is_none() && self.failed.is_none() {
            if let Some(err) = &self.error {
                return status(format!("queries unavailable: {err}"));
            }
            return status("loading queries…");
        }
        let mut col = div().flex().flex_col().gap(px(16.0)).w_full();
        if let Some(err) = &self.error {
            col = col.child(
                div()
                    .text_size(px(12.0))
                    .text_color(bezel::theme::ink(0.6))
                    .child(format!("partial: {err}")),
            );
        }
        col.child(section("Running", self.running.as_ref(), false))
            .child(section("Slow", self.slow.as_ref(), false))
            .child(section("Failed", self.failed.as_ref(), true))
    }
}
