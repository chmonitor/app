//! Page routing.

use gpui::{App, FontWeight, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, IconName};

pub mod health;
pub mod merges;
pub mod overview;
pub mod queries;
pub mod replicas;
pub mod settings;
pub mod tables;
pub mod traffic;

/// Every sidebar destination, in keyboard-shortcut order (1-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Queries,
    Merges,
    Replicas,
    Health,
    Tables,
    Traffic,
    Connect,
    Settings,
}

impl Page {
    /// Sidebar order == keyboard shortcut order.
    pub const ALL: [Page; 8] = [
        Page::Overview,
        Page::Queries,
        Page::Merges,
        Page::Replicas,
        Page::Health,
        Page::Tables,
        Page::Traffic,
        Page::Connect,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Queries => "Queries",
            Page::Merges => "Merges",
            Page::Replicas => "Replicas",
            Page::Health => "Health",
            Page::Tables => "Tables",
            Page::Traffic => "Traffic",
            Page::Connect => "Connect",
            Page::Settings => "Settings",
        }
    }

    /// Pages whose fetch takes the selected time range.
    pub fn uses_range(self) -> bool {
        matches!(self, Page::Overview | Page::Queries | Page::Traffic)
    }

    /// ClickHouse-only pages are hidden on a Postgres host.
    pub fn available(self, engine: chm_core::SourceEngine) -> bool {
        match engine {
            chm_core::SourceEngine::Postgres => !matches!(self, Page::Merges | Page::Traffic),
            _ => true,
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Page::Overview => IconName::LayoutDashboard,
            Page::Queries => IconName::Search,
            Page::Merges => IconName::Replace,
            Page::Replicas => IconName::Copy,
            Page::Health => IconName::Heart,
            Page::Tables => IconName::File,
            Page::Traffic => IconName::ChartPie,
            Page::Connect => IconName::Globe,
            Page::Settings => IconName::Settings,
        }
    }
}

pub(crate) fn status(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .text_sm()
        .child(text.into())
}

pub(crate) fn heading(title: &str) -> gpui::Div {
    div()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(title.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pages_have_stable_indexes_and_titles() {
        assert_eq!(Page::ALL.len(), 8);
        for (i, page) in Page::ALL.iter().enumerate() {
            assert_eq!(page.index(), i);
            assert!(!page.title().is_empty());
        }
        assert_eq!(Page::Overview.title(), "Overview");
        assert_eq!(Page::Connect.title(), "Connect");
        assert_eq!(Page::Connect.index(), 7);
        assert!(Page::Overview.uses_range());
        assert!(Page::Queries.uses_range());
        assert!(Page::Traffic.uses_range());
        assert!(!Page::Merges.uses_range());
        assert!(!Page::Connect.uses_range());
        assert_eq!(Page::Settings.title(), "Settings");
        assert!(!Page::ALL.contains(&Page::Settings));
        assert!(!Page::Merges.available(chm_core::SourceEngine::Postgres));
        assert!(Page::Queries.available(chm_core::SourceEngine::Postgres));
        assert!(Page::Merges.available(chm_core::SourceEngine::ClickHouse));
    }
}
