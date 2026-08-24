//! Layout density and which Overview metrics to show.
//!
//! Compact is the default: tighter chrome, the four dash.chmonitor.dev
//! KPI cards (active queries, schema, storage, uptime), optional sparkline.
//! Comfortable restores the roomier dashboard. Both are `[ui]` keys.

use crate::config::load_config;

/// Card / chrome spacing. Unknown or missing config → Compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Compact,
    Comfortable,
}

impl Density {
    pub const ALL: [Density; 2] = [Density::Compact, Density::Comfortable];

    pub fn from_cfg(s: Option<&str>) -> Self {
        match s.map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("comfortable") => Self::Comfortable,
            _ => Self::Compact,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Comfortable => "comfortable",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Compact => "tighter cards, key metrics first",
            Self::Comfortable => "roomier dashboard",
        }
    }

    pub fn current() -> Self {
        Self::from_cfg(load_config().ui.density.as_deref())
    }

    pub fn font_size(self) -> f32 {
        match self {
            Self::Compact => 13.0,
            Self::Comfortable => 14.0,
        }
    }

    pub fn mono_font_size(self) -> f32 {
        match self {
            Self::Compact => 11.0,
            Self::Comfortable => 12.0,
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            Self::Compact => 6.0,
            Self::Comfortable => 10.0,
        }
    }

    pub fn radius_lg(self) -> f32 {
        self.radius() + 2.0
    }

    pub fn card_pad(self) -> f32 {
        match self {
            Self::Compact => 8.0,
            Self::Comfortable => 14.0,
        }
    }

    pub fn card_gap(self) -> f32 {
        match self {
            Self::Compact => 6.0,
            Self::Comfortable => 10.0,
        }
    }

    pub fn card_value(self) -> f32 {
        match self {
            Self::Compact => 18.0,
            Self::Comfortable => 24.0,
        }
    }

    pub fn card_min_w(self) -> f32 {
        match self {
            Self::Compact => 108.0,
            Self::Comfortable => 140.0,
        }
    }

    pub fn metrics_per_row(self) -> usize {
        match self {
            Self::Compact | Self::Comfortable => 4,
        }
    }

    pub fn chart_h(self) -> f32 {
        match self {
            Self::Compact => 128.0,
            Self::Comfortable => 200.0,
        }
    }

    pub fn content_pad(self) -> f32 {
        match self {
            Self::Compact => 12.0,
            Self::Comfortable => 16.0,
        }
    }

    pub fn table_px(self) -> f32 {
        match self {
            Self::Compact => 8.0,
            Self::Comfortable => 12.0,
        }
    }

    pub fn table_py(self) -> f32 {
        match self {
            Self::Compact => 4.0,
            Self::Comfortable => 8.0,
        }
    }
}

/// One Overview tile. Ids are the `[ui].overview_metrics` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewMetric {
    Running,
    Schema,
    Disk,
    Uptime,
    Qps,
    Slow,
    Failed,
    Merges,
    Replicas,
    Tables,
    Parts,
    Version,
}

impl OverviewMetric {
    pub const ALL: [OverviewMetric; 12] = [
        Self::Running,
        Self::Schema,
        Self::Disk,
        Self::Uptime,
        Self::Qps,
        Self::Slow,
        Self::Failed,
        Self::Merges,
        Self::Replicas,
        Self::Tables,
        Self::Parts,
        Self::Version,
    ];

    /// Default visible set: the four dash.chmonitor.dev overview KPIs.
    pub const DEFAULT: [OverviewMetric; 4] =
        [Self::Running, Self::Schema, Self::Disk, Self::Uptime];

    pub fn id(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Schema => "schema",
            Self::Disk => "disk",
            Self::Uptime => "uptime",
            Self::Qps => "qps",
            Self::Slow => "slow",
            Self::Failed => "failed",
            Self::Merges => "merges",
            Self::Replicas => "replicas",
            Self::Tables => "tables",
            Self::Parts => "parts",
            Self::Version => "version",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Active Queries",
            Self::Schema => "Schema",
            Self::Disk => "Storage",
            Self::Uptime => "Uptime",
            Self::Qps => "queries / sec",
            Self::Slow => "slow · 24h",
            Self::Failed => "failed · 24h",
            Self::Merges => "active merges",
            Self::Replicas => "replicas",
            Self::Tables => "tables",
            Self::Parts => "parts",
            Self::Version => "version",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|m| m.id() == s)
    }
}

/// Resolve the Overview tile list. Empty or all-unknown → the default six.
pub fn visible_metrics(ids: &[String]) -> Vec<OverviewMetric> {
    let parsed: Vec<OverviewMetric> = ids
        .iter()
        .filter_map(|s| OverviewMetric::from_id(s))
        .collect();
    if parsed.is_empty() {
        OverviewMetric::DEFAULT.to_vec()
    } else {
        parsed
    }
}

pub fn default_metric_ids() -> Vec<String> {
    OverviewMetric::DEFAULT
        .iter()
        .map(|m| m.id().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_parses_with_compact_default() {
        assert_eq!(Density::from_cfg(None), Density::Compact);
        assert_eq!(Density::from_cfg(Some("compact")), Density::Compact);
        assert_eq!(Density::from_cfg(Some("COMFORTABLE")), Density::Comfortable);
        assert_eq!(Density::from_cfg(Some("nope")), Density::Compact);
        assert_eq!(Density::Compact.as_str(), "compact");
        assert_eq!(Density::Compact.metrics_per_row(), 4);
        assert!(Density::Compact.card_pad() < Density::Comfortable.card_pad());
        assert!(Density::Compact.chart_h() < Density::Comfortable.chart_h());
    }

    #[test]
    fn visible_metrics_falls_back_to_dashboard_kpis() {
        assert_eq!(visible_metrics(&[]), OverviewMetric::DEFAULT);
        assert_eq!(
            visible_metrics(&["nope".into(), "also-nope".into()]),
            OverviewMetric::DEFAULT
        );
        assert_eq!(
            visible_metrics(&["qps".into(), "disk".into()]),
            vec![OverviewMetric::Qps, OverviewMetric::Disk]
        );
        assert_eq!(
            OverviewMetric::from_id("schema"),
            Some(OverviewMetric::Schema)
        );
        assert!(OverviewMetric::from_id("nope").is_none());
        assert_eq!(
            default_metric_ids(),
            ["running", "schema", "disk", "uptime"]
        );
    }
}
