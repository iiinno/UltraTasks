mod agenda;
mod editor;
mod sidebar;
mod task_list;
mod today;

use crate::theme::{CONTROL_WEIGHT, Tokens};
use gpui_kit::{
    base::FocusableExt,
    component::{
        Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::*,
    *,
};

pub(crate) fn focus_outline(t: Tokens) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), t.focus_ring).spread_radius(px(1.))]
}

pub(crate) fn button(t: Tokens, id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .compact()
        .h(px(t.control_height))
        .font_weight(CONTROL_WEIGHT)
        .relative()
        .focus_ring(false)
        .focus_visible(move |s| s.shadow(focus_outline(t)))
        .when(t.canvas == t.surface_hover, |button| {
            button.group("desktop-control").child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(t.control_radius))
                    .border_1()
                    .border_color(transparent_black())
                    .group_hover("desktop-control", move |s| s.border_color(t.focus_ring)),
            )
        })
}

pub(crate) fn validation_message(
    id: impl Into<ElementId>,
    message: impl Into<SharedString>,
    t: Tokens,
) -> Stateful<Div> {
    let message = message.into();
    div()
        .id(id)
        .role(Role::Status)
        .aria_label(message.clone())
        .w_full()
        .px_2()
        .py_1()
        .rounded(px(t.control_radius))
        .text_size(px(t.secondary_size))
        .line_height(px(t.secondary_line))
        .bg(t.error_background)
        .text_color(t.error_text)
        .child(message)
}

pub(crate) fn input_frame(id: EntityId, focus: &FocusHandle, t: Tokens) -> Stateful<Div> {
    div()
        .id(("input-focus", id))
        .track_focus(focus)
        .rounded(px(t.control_radius))
        .focus_visible(move |s| s.shadow(focus_outline(t)))
}
