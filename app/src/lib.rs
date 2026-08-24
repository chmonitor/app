//! chm-app — GPUI + bezel desktop client for chmonitor.
//!
//! AGENT D OWNS shell.rs / main.rs / connect.rs / pages/ wiring.
//! AGENT E OWNS widgets/ (chart, table, metric card).
//! AGENT F/G/H fill pages/. Keep all gpui types via `bezel::gpui`.

pub mod config;
pub mod connect;
pub mod pages;
pub mod shell;
pub mod widgets;
