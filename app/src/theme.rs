//! Brand theme matching the chmonitor.dev dashboard (Rhea: indigo primary,
//! amber charts, 10px radius, SF/system UI + Menlo).

use gpui::{App, Hsla, rgb};
use gpui_component::{Theme, ThemeMode};

use crate::density::Density;

fn hx(v: u32) -> Hsla {
    rgb(v).into()
}

/// Paint dashboard colors onto the active gpui-component theme.
pub fn apply_brand(cx: &mut App) {
    let dark = Theme::global(cx).is_dark();
    let density = Density::current();
    let theme = Theme::global_mut(cx);
    theme.font_size = gpui::px(density.font_size());
    theme.mono_font_size = gpui::px(density.mono_font_size());
    theme.radius = gpui::px(density.radius());
    theme.radius_lg = gpui::px(density.radius_lg());
    theme.mono_font_family = "Menlo".into();
    if dark {
        paint_dark(theme);
    } else {
        paint_light(theme);
    }
    Theme::sync_base(cx);
}

fn paint_light(theme: &mut Theme) {
    theme.background = hx(0xffffff);
    theme.foreground = hx(0x252525);
    theme.secondary = hx(0xf4f4f7);
    theme.secondary_foreground = hx(0x252525);
    theme.muted = hx(0xf4f4f5);
    theme.muted_foreground = hx(0x737373);
    theme.accent = hx(0xf4f4f5);
    theme.accent_foreground = hx(0x252525);
    theme.primary = hx(0x4f46e5);
    theme.primary_foreground = hx(0xf5f7ff);
    theme.primary_hover = hx(0x4338ca);
    theme.primary_active = hx(0x3730a3);
    theme.button_primary = theme.primary;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.button_primary_hover = theme.primary_hover;
    theme.border = hx(0xe5e5e5);
    theme.input = hx(0xe5e5e5);
    theme.ring = hx(0xa5b4fc);
    theme.danger = hx(0xe11d48);
    theme.danger_foreground = hx(0xffffff);
    theme.warning = hx(0xd97706);
    theme.green = hx(0x16a34a);
    theme.sidebar = hx(0xfafafa);
    theme.sidebar_foreground = hx(0x252525);
    theme.sidebar_accent = hx(0xf4f4f5);
    theme.sidebar_accent_foreground = hx(0x252525);
    theme.sidebar_border = hx(0xe5e5e5);
    theme.sidebar_primary = hx(0x4f46e5);
    theme.sidebar_primary_foreground = hx(0xf5f7ff);
    theme.chart_1 = hx(0xeab308);
    theme.chart_2 = hx(0xf59e0b);
    theme.chart_3 = hx(0xf97316);
    theme.chart_4 = hx(0xea580c);
    theme.chart_5 = hx(0xc2410c);
    theme.skeleton = hx(0xe5e5e5);
    theme.popover = hx(0xffffff);
    theme.popover_foreground = hx(0x252525);
    theme.title_bar = hx(0xfafafa);
    theme.title_bar_border = hx(0xe5e5e5);
    theme.status_bar = hx(0xfafafa);
    theme.status_bar_border = hx(0xe5e5e5);
}

fn paint_dark(theme: &mut Theme) {
    theme.background = hx(0x171717);
    theme.foreground = hx(0xfafafa);
    theme.secondary = hx(0x2a2a2e);
    theme.secondary_foreground = hx(0xfafafa);
    theme.muted = hx(0x262626);
    theme.muted_foreground = hx(0xa1a1aa);
    theme.accent = hx(0x262626);
    theme.accent_foreground = hx(0xfafafa);
    theme.primary = hx(0x818cf8);
    theme.primary_foreground = hx(0x1e1b4b);
    theme.primary_hover = hx(0xa5b4fc);
    theme.primary_active = hx(0x6366f1);
    theme.button_primary = theme.primary;
    theme.button_primary_foreground = theme.primary_foreground;
    theme.button_primary_hover = theme.primary_hover;
    theme.border = hx(0x3f3f46);
    theme.input = hx(0x3f3f46);
    theme.ring = hx(0x818cf8);
    theme.danger = hx(0xfb7185);
    theme.danger_foreground = hx(0x1c1917);
    theme.warning = hx(0xfbbf24);
    theme.green = hx(0x4ade80);
    theme.sidebar = hx(0x2a2a2a);
    theme.sidebar_foreground = hx(0xfafafa);
    theme.sidebar_accent = hx(0x3f3f46);
    theme.sidebar_accent_foreground = hx(0xfafafa);
    theme.sidebar_border = hx(0x3f3f46);
    theme.sidebar_primary = hx(0x818cf8);
    theme.sidebar_primary_foreground = hx(0x1e1b4b);
    theme.chart_1 = hx(0xeab308);
    theme.chart_2 = hx(0xf59e0b);
    theme.chart_3 = hx(0xf97316);
    theme.chart_4 = hx(0xea580c);
    theme.chart_5 = hx(0xc2410c);
    theme.skeleton = hx(0x3f3f46);
    theme.popover = hx(0x2a2a2a);
    theme.popover_foreground = hx(0xfafafa);
    theme.title_bar = hx(0x1c1c1c);
    theme.title_bar_border = hx(0x3f3f46);
    theme.status_bar = hx(0x1c1c1c);
    theme.status_bar_border = hx(0x3f3f46);
}

pub fn current_mode(cx: &App) -> ThemeMode {
    if Theme::global(cx).is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}
