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
        .border_1()
        .border_color(transparent_black())
        .focus_visible(move |s| s.border_color(t.focus_ring))
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

pub(crate) fn menu_trigger(
    id: &'static str,
    trigger: Button,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> Button {
    let state = window.use_keyed_state(id, cx, |_, cx| cx.focus_handle());
    let focus = state.read(cx).clone();
    trigger
        .track_focus(&focus)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            if !disabled {
                cx.stop_propagation();
                focus.dispatch_action(
                    &gpui_kit::base::actions::Confirm { secondary: false },
                    window,
                    cx,
                );
            }
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
        .border_1()
        .border_color(transparent_black())
        .focus_visible(move |s| s.border_color(t.focus_ring))
}
