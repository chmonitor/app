//! macOS-native chrome: system UI font, SF/Menlo mono, HIG colors.
//!
//! Accent is system blue (`#007AFF` / `#0A84FF`). Surfaces follow window /
//! sidebar / separator labels from Apple's Human Interface palette so the
//! desktop client reads like Activity Monitor rather than the web dashboard.

use gpui::{App, Hsla, rgb};
use gpui_component::{Theme, ThemeMode};

use crate::density::Density;

fn hx(v: u32) -> Hsla {
    rgb(v).into()
}

/// Apply system type and macOS semantic colors onto the active theme.
pub fn apply_brand(cx: &mut App) {
    let dark = Theme::global(cx).is_dark();
    let density = Density::current();
    let theme = Theme::global_mut(cx);
    theme.font_family = ".SystemUIFont".into();
    theme.font_size = gpui::px(density.font_size());
    theme.mono_font_family = if cfg!(target_os = "macos") {
        "Menlo".into()
    } else {
        "ui-monospace".into()
    };
    theme.mono_font_size = gpui::px(density.mono_font_size());
    theme.radius = gpui::px(density.radius());
    theme.radius_lg = gpui::px(density.radius_lg());
    if dark {
        paint_dark(theme);
    } else {
        paint_light(theme);
    }
    Theme::sync_base(cx);
}

fn paint_light(theme: &mut Theme) {
    let blue = hx(0x007AFF);
    let label = hx(0x1D1D1F);
    let fill = hx(0xF2F2F7);
    let hairline = hx(0xD1D1D6);

    theme.background = hx(0xFFFFFF);
    theme.foreground = label;
    theme.secondary = fill;
    theme.secondary_foreground = label;
    theme.secondary_hover = hx(0xE5E5EA);
    theme.secondary_active = hx(0xD1D1D6);
    theme.muted = fill;
    theme.muted_foreground = hx(0x6E6E73);
    theme.accent = hx(0xE5E5EA);
    theme.accent_foreground = label;
    theme.primary = blue;
    theme.primary_foreground = hx(0xFFFFFF);
    theme.primary_hover = hx(0x0066D6);
    theme.primary_active = hx(0x0055C4);
    theme.button_primary = theme.primary;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.button_primary_hover = theme.primary_hover;
    theme.border = hairline;
    theme.input = hairline;
    theme.ring = blue;
    theme.selection = blue.opacity(0.28);
    theme.danger = hx(0xFF3B30);
    theme.danger_foreground = hx(0xFFFFFF);
    theme.warning = hx(0xFF9F0A);
    theme.warning_foreground = hx(0x1D1D1F);
    theme.success = hx(0x34C759);
    theme.success_foreground = hx(0xFFFFFF);
    theme.green = hx(0x34C759);
    theme.red = hx(0xFF3B30);
    theme.blue = blue;
    theme.sidebar = hx(0xF5F5F7);
    theme.sidebar_foreground = label;
    theme.sidebar_accent = hx(0xE5E5EA);
    theme.sidebar_accent_foreground = label;
    theme.sidebar_border = hairline;
    theme.sidebar_primary = blue;
    theme.sidebar_primary_foreground = hx(0xFFFFFF);
    theme.chart_1 = blue;
    theme.chart_2 = hx(0x5AC8FA);
    theme.chart_3 = hx(0x64D2FF);
    theme.chart_4 = hx(0x0A84FF);
    theme.chart_5 = hx(0x0055C4);
    theme.skeleton = hx(0xE5E5EA);
    theme.popover = hx(0xFFFFFF);
    theme.popover_foreground = label;
    theme.title_bar = hx(0xF5F5F7);
    theme.title_bar_border = hairline;
    theme.status_bar = hx(0xF5F5F7);
    theme.status_bar_border = hairline;
    theme.switch = hx(0xD1D1D6);
    theme.switch_thumb = hx(0xFFFFFF);
    theme.tab_bar = fill;
    theme.tab_bar_segmented = fill;
    theme.tab = Hsla::transparent_black();
    theme.tab_active = hx(0xFFFFFF);
    theme.tab_active_foreground = label;
    theme.tab_foreground = hx(0x6E6E73);
    theme.list_hover = hx(0xE5E5EA);
    theme.list_active = blue.opacity(0.12);
    theme.list_active_border = blue;
    theme.table_head = fill;
    theme.table_head_foreground = hx(0x6E6E73);
    theme.table_row_border = hairline;
    theme.table_hover = hx(0xF2F2F7);
    theme.progress_bar = blue;
    theme.scrollbar_thumb = hx(0xC7C7CC);
    theme.overlay = hx(0x000000).opacity(0.18);
}

fn paint_dark(theme: &mut Theme) {
    let blue = hx(0x0A84FF);
    let label = hx(0xF5F5F7);
    let fill = hx(0x2C2C2E);
    let hairline = hx(0x3A3A3C);

    theme.background = hx(0x1C1C1E);
    theme.foreground = label;
    theme.secondary = fill;
    theme.secondary_foreground = label;
    theme.secondary_hover = hx(0x3A3A3C);
    theme.secondary_active = hx(0x48484A);
    theme.muted = fill;
    theme.muted_foreground = hx(0x8E8E93);
    theme.accent = hx(0x3A3A3C);
    theme.accent_foreground = label;
    theme.primary = blue;
    theme.primary_foreground = hx(0xFFFFFF);
    theme.primary_hover = hx(0x409CFF);
    theme.primary_active = hx(0x0070E0);
    theme.button_primary = theme.primary;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.button_primary_hover = theme.primary_hover;
    theme.border = hairline;
    theme.input = hairline;
    theme.ring = blue;
    theme.selection = blue.opacity(0.40);
    theme.danger = hx(0xFF453A);
    theme.danger_foreground = hx(0xFFFFFF);
    theme.warning = hx(0xFF9F0A);
    theme.warning_foreground = hx(0x1C1C1E);
    theme.success = hx(0x30D158);
    theme.success_foreground = hx(0x1C1C1E);
    theme.green = hx(0x30D158);
    theme.red = hx(0xFF453A);
    theme.blue = blue;
    theme.sidebar = hx(0x2C2C2E);
    theme.sidebar_foreground = label;
    theme.sidebar_accent = hx(0x3A3A3C);
    theme.sidebar_accent_foreground = label;
    theme.sidebar_border = hairline;
    theme.sidebar_primary = blue;
    theme.sidebar_primary_foreground = hx(0xFFFFFF);
    theme.chart_1 = blue;
    theme.chart_2 = hx(0x64D2FF);
    theme.chart_3 = hx(0x5AC8FA);
    theme.chart_4 = hx(0x007AFF);
    theme.chart_5 = hx(0x409CFF);
    theme.skeleton = hx(0x3A3A3C);
    theme.popover = hx(0x2C2C2E);
    theme.popover_foreground = label;
    theme.title_bar = hx(0x2C2C2E);
    theme.title_bar_border = hairline;
    theme.status_bar = hx(0x2C2C2E);
    theme.status_bar_border = hairline;
    theme.switch = hx(0x39393D);
    theme.switch_thumb = hx(0xFFFFFF);
    theme.tab_bar = fill;
    theme.tab_bar_segmented = fill;
    theme.tab = Hsla::transparent_black();
    theme.tab_active = hx(0x3A3A3C);
    theme.tab_active_foreground = label;
    theme.tab_foreground = hx(0x8E8E93);
    theme.list_hover = hx(0x3A3A3C);
    theme.list_active = blue.opacity(0.22);
    theme.list_active_border = blue;
    theme.table_head = fill;
    theme.table_head_foreground = hx(0x8E8E93);
    theme.table_row_border = hairline;
    theme.table_hover = hx(0x3A3A3C);
    theme.progress_bar = blue;
    theme.scrollbar_thumb = hx(0x636366);
    theme.overlay = hx(0x000000).opacity(0.45);
}

pub fn current_mode(cx: &App) -> ThemeMode {
    if Theme::global(cx).is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}
