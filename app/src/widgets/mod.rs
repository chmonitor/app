//! Reusable dashboard widgets: metric cards, line charts, data tables and
//! their shared geometry math. Owned by Agent E; all gpui types flow through
//! `bezel::gpui`.

pub mod cards;
pub mod chart;
pub mod geometry;
pub mod table;

pub use cards::metric_card;
pub use chart::{NamedSeries, line_chart};
pub use table::{CellVal, Column, data_table};
