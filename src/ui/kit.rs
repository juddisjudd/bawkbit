use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{Palette, palette};

/// Every text box in the app, without the border that lights up on focus.
pub fn input(state: &Entity<InputState>) -> Input {
    Input::new(state).focus_bordered(false)
}

/// bawkterm's plain button: no fill and a faint border until hovered.
pub fn button(id: impl Into<ElementId>, cx: &App) -> Button {
    let p = palette(cx);
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(p.text_strong)
                .hover(p.bg_hover)
                .active(p.border_weak),
        )
        .border_1()
        .border_color(p.border_weak)
}

/// The plain button in red, for a second choice that destroys something.
pub fn danger_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let p = palette(cx);
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(p.danger)
                .hover(p.bg_hover)
                .active(p.border_weak),
        )
        .border_1()
        .border_color(p.border_weak)
}

pub fn icon_button(id: impl Into<ElementId>, icon: IconName, tooltip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(Icon::new(icon))
        .tooltip(tooltip)
}

/// A row of options where one is picked; fill it with `segment`s.
pub fn segmented(p: &Palette) -> Div {
    div()
        .flex()
        .flex_none()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(4.))
        .border_1()
        .border_color(p.border_weak)
}

pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    p: &Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.))
        .py(px(3.))
        .rounded(px(3.))
        .cursor_pointer()
        .text_color(if active { p.text_strong } else { p.text_weak })
        .when(active, |this| this.bg(p.bg_hover))
        .hover(|style| style.text_color(p.text_strong))
        .child(label.into())
}

pub fn progress_bar(fraction: f32, color: Hsla, p: &Palette) -> Div {
    div()
        .h(px(3.))
        .w_full()
        .rounded_full()
        .bg(p.border_weak)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(relative(fraction.clamp(0.0, 1.0))),
        )
}

pub fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    p: &Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(4.))
        .border_1()
        .text_size(px(12.))
        .cursor_pointer()
        .map(|this| {
            if active {
                this.border_color(p.yolk)
                    .bg(p.yolk_dim)
                    .text_color(p.text_strong)
            } else {
                this.border_color(p.border_weak)
                    .text_color(p.text_weak)
                    .hover(|style| style.text_color(p.text_strong).bg(p.bg_weak))
            }
        })
        .child(label.into())
}

pub fn empty_state(body: &str, action: impl IntoElement, p: &Palette) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .text_color(p.text_weak)
        .child(
            svg()
                .path(crate::assets::MARK)
                .size(px(44.))
                .text_color(p.border_weak),
        )
        .child(div().max_w(px(420.)).text_center().child(body.to_string()))
        .child(action)
}

pub fn dot(color: Hsla) -> Div {
    div().size(px(7.)).flex_none().rounded_full().bg(color)
}

pub fn truncate(text: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

/// A small weak caption over a value, for the few places a value cannot explain itself.
pub fn stat(label: &'static str, value: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .child(
            div()
                .text_size(px(11.))
                .text_color(p.text_weaker)
                .child(label),
        )
        .child(truncate(value).text_color(p.text_strong))
}
