//! Page routing. AGENT D owns mod.rs; Agents F/G/H own the individual page
//! files and will replace the placeholder bodies in shell.rs's `content`.

use bezel::gpui::{AnyElement, FontWeight, SharedString, div, prelude::*, px};

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

    /// Sidebar glyph. Text markers until Agent E's icon widget lands; the
    /// sidebar renders whatever this returns, so swapping in real icons is a
    /// one-file change.
    pub fn icon(self) -> AnyElement {
        let glyph: &str = match self {
            Page::Overview => "◧",
            Page::Queries => "⌕",
            Page::Merges => "⇄",
            Page::Replicas => "⑃",
            Page::Health => "♥",
            Page::Tables => "▤",
            Page::Traffic => "↕",
            Page::Connect => "⌁",
            Page::Settings => "⚙",
        };
        div()
            .w(px(16.0))
            .text_size(px(13.0))
            .child(SharedString::from(glyph.to_string()))
            .into_any_element()
    }
}

pub(crate) fn status(text: impl Into<SharedString>) -> bezel::gpui::Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_color(bezel::theme::ink(0.45))
        .text_size(px(13.0))
        .child(text.into())
}

pub(crate) fn heading(title: &str) -> bezel::gpui::Div {
    div()
        .text_size(px(13.0))
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
    }
}
