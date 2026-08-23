//! Page routing. AGENT D owns mod.rs; Agents F/G/H own the individual page
//! files and will replace the placeholder bodies in shell.rs's `content`.

use bezel::gpui::{AnyElement, SharedString, div, prelude::*, px};

pub mod overview;

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
        }
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
        };
        div()
            .w(px(16.0))
            .text_size(px(13.0))
            .child(SharedString::from(glyph.to_string()))
            .into_any_element()
    }
}
