//! Application-owned presentation on gpui-base primitives.
//!
//! Base owns focus, keyboard, and accessibility. Theme tokens and layout
//! stay here so the product is not locked to gpui-component's default look.

use gpui::{App, ElementId, SharedString, div, prelude::*, px, relative};
use gpui_base::{Button, Radio, RadioGroup, Switch, SwitchThumb, SwitchTrack, Toggle, ToggleGroup};
use gpui_component::ActiveTheme as _;

pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    Button::new(id)
        .px_3()
        .h_8()
        .flex()
        .items_center()
        .rounded(cx.theme().radius)
        .bg(cx.theme().primary)
        .text_color(cx.theme().primary_foreground)
        .hover(|s| s.opacity(0.9))
        .child(label.into())
}

pub fn ghost_button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Button {
    Button::new(id)
        .px_3()
        .h_8()
        .flex()
        .items_center()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .hover(|s| s.bg(cx.theme().accent))
        .child(label.into())
}

/// A labeled radio option with an optional hint line.
pub fn choice_radio(
    id: impl Into<ElementId>,
    checked: bool,
    label: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    cx: &App,
) -> Radio {
    let primary = cx.theme().primary;
    let muted = cx.theme().muted_foreground;
    Radio::new(id)
        .checked(checked)
        .flex()
        .items_start()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if checked {
            cx.theme().primary
        } else {
            cx.theme().border
        })
        .bg(if checked {
            cx.theme().accent
        } else {
            cx.theme().background
        })
        .child(
            div()
                .mt(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .size(px(14.))
                .rounded_full()
                .border_1()
                .border_color(primary)
                .when(checked, |dot| {
                    dot.child(div().size(px(6.)).rounded_full().bg(primary))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().child(label.into()))
                .child(div().text_xs().text_color(muted).child(hint.into())),
        )
}

pub fn radio_group(id: impl Into<ElementId>) -> RadioGroup {
    RadioGroup::new(id).flex().flex_col().gap_2()
}

pub fn range_toggle(
    id: impl Into<ElementId>,
    pressed: bool,
    label: impl Into<SharedString>,
    cx: &App,
) -> Toggle {
    Toggle::new(id)
        .pressed(pressed)
        .px_2()
        .h_6()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .line_height(relative(1.))
        .rounded(cx.theme().radius)
        .when(pressed, |t| {
            t.bg(cx.theme().primary)
                .text_color(cx.theme().primary_foreground)
        })
        .when(!pressed, |t| t.text_color(cx.theme().muted_foreground))
        .hover(|s| s.bg(cx.theme().accent))
        .child(label.into())
}

pub fn range_group(id: impl Into<ElementId>) -> ToggleGroup {
    ToggleGroup::new(id).flex().items_center().gap_1()
}

/// Compact on/off switch styled from theme tokens.
pub fn theme_switch(id: impl Into<ElementId>, checked: bool, cx: &App) -> Switch {
    let on = cx.theme().primary;
    let off = cx.theme().border;
    let thumb = cx.theme().background;
    Switch::new(id).checked(checked).child(
        SwitchTrack::new("switch-track")
            .checked(checked)
            .w(px(36.))
            .h(px(20.))
            .p(px(2.))
            .rounded_full()
            .bg(if checked { on } else { off })
            .child(
                SwitchThumb::new(checked)
                    .size_4()
                    .rounded_full()
                    .bg(thumb)
                    .ml(if checked { px(16.) } else { px(0.) }),
            ),
    )
}
