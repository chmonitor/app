//! Reusable dashboard widgets: metric cards, line charts, data tables and
//! their shared geometry math.

pub mod cards;
pub mod chart;
pub mod controls;
pub mod geometry;
pub mod skeleton;
pub mod table;

pub use cards::metric_card;
pub use chart::{NamedSeries, line_chart};
pub use table::{CellVal, Column, data_table};
