use crate::app::*;
use gpui_kit::{
    assets::IconName,
    component::{Icon, Selectable, Sizable, button::Button},
};

fn sidebar_item(
    t: Tokens,
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    rail: bool,
    selected: bool,
) -> Button {
    let label = label.into();
    button(t, id)
        .w_full()
        .h(px(t.nav_height))
        .my(px(1.))
        .px(px(7.))
        .rounded(px(8.))
        .text_color(t.text_secondary)
        .selected(selected)
        .accessibility_label(label.clone())
        .tooltip(label.clone())
        .when(selected, |button| {
            button
                .bg(if t.canvas == t.surface_hover {
                    t.selection_background
                } else {
                    t.surface_hover
                })
                .text_color(if t.canvas == t.surface_hover {
                    t.selection_text
                } else {
                    t.text_primary
                })
        })
        .child(
            div()
                .h_flex()
                .w_full()
                .min_w_0()
                .text_size(px(13.))
                .font_weight(FontWeight(510.))
                .gap(px(8.))
                .when(rail, |d| d.justify_center())
                .child(Icon::new(icon).with_size(px(16.)))
                .when(!rail, |d| {
                    d.child(div().min_w_0().text_ellipsis().child(label))
                }),
        )
}

impl Desktop {
    pub(crate) fn sidebar(&self, rail: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = self.tokens;
        let mut lists = div()
            .id("sidebar-lists")
            .v_flex()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .when(!rail, |d| {
                d.child(
                    div()
                        .h(px(t.nav_height))
                        .h_flex()
                        .px(px(7.))
                        .text_size(px(12.))
                        .text_color(t.text_secondary)
                        .child("リスト"),
                )
            });
        for collection in &self.collections {
            let Collection::List(list) = collection else {
                continue;
            };
            let id = list.key.id;
            let selected = !self.calendar_view && self.filters.list == Some(id);
            lists = lists.child(
                sidebar_item(
                    t,
                    SharedString::from(format!("sidebar-list-{id}")),
                    IconName::List,
                    list.data.as_str().to_owned(),
                    rail,
                    selected,
                )
                .toggled(selected)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.navigate(Navigation::List(Some(id)), window, cx)
                })),
            );
        }
        if self.collections_previous.is_some() || self.collections_next.is_some() {
            lists = lists.child(
                div()
                    .h_flex()
                    .when(rail, |d| d.flex_col())
                    .gap_1()
                    .child(
                        button(t, "sidebar-lists-before")
                            .icon(IconName::ChevronUp)
                            .h(px(t.nav_height))
                            .w(px(t.nav_height))
                            .tooltip("前のリスト")
                            .accessibility_label("前のリスト")
                            .disabled(self.collections_previous.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(cursor) = this.collections_previous.clone() {
                                    this.load_collections(PagePosition::Before(cursor), window, cx);
                                }
                            })),
                    )
                    .child(
                        button(t, "sidebar-lists-after")
                            .icon(IconName::ChevronDown)
                            .h(px(t.nav_height))
                            .w(px(t.nav_height))
                            .tooltip("次のリスト")
                            .accessibility_label("次のリスト")
                            .disabled(self.collections_next.is_none())
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(cursor) = this.collections_next.clone() {
                                    this.load_collections(PagePosition::After(cursor), window, cx);
                                }
                            })),
                    ),
            );
        }
        div()
            .id("sidebar")
            .role(Role::Navigation)
            .aria_label("タスクと時間割")
            .v_flex()
            .w(px(if rail { 48. } else { t.sidebar_width }))
            .h_full()
            .min_h_0()
            .flex_shrink_0()
            .bg(t.sidebar)
            .text_color(t.text_secondary)
            .child(
                div()
                    .h_flex()
                    .min_h(px(t.header_height))
                    .flex_shrink_0()
                    .px(px(12.))
                    .gap(px(6.))
                    .when(rail, |d| d.flex_col().px(px(8.)).py(px(8.)))
                    .child(
                        div()
                            .h_flex()
                            .h(px(t.nav_height))
                            .flex_1()
                            .min_w_0()
                            .px(px(8.))
                            .text_size(px(14.))
                            .font_weight(FontWeight(590.))
                            .text_color(t.text_primary)
                            .when(rail, |d| d.flex_none().px_0().justify_center())
                            .child(
                                div()
                                    .truncate()
                                    .child(if rail { "UT" } else { "UltraTasks" }),
                            ),
                    )
                    .child(
                        button(t, "sidebar-search")
                            .icon(IconName::Search)
                            .w(px(t.nav_height))
                            .h(px(t.nav_height))
                            .flex_shrink_0()
                            .rounded(px(9999.))
                            .text_color(t.text_secondary)
                            .tooltip("タスクを検索 Ctrl+K")
                            .accessibility_label("タスクを検索")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate(Navigation::SearchInput, window, cx)
                            })),
                    )
                    .child(
                        button(t, "sidebar-new-task")
                            .icon(IconName::SquarePen)
                            .w(px(t.nav_height))
                            .h(px(t.nav_height))
                            .flex_shrink_0()
                            .rounded(px(9999.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.surface_hover)
                            .text_color(t.text_primary)
                            .tooltip("タスクを追加 Ctrl+N")
                            .accessibility_label("タスクを追加")
                            .disabled(self.info.is_none() || self.recovery)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate(Navigation::QuickInput, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(12.))
                    .when(rail, |d| d.px(px(8.)))
                    .child(
                        sidebar_item(
                            t,
                            "sidebar-tasks",
                            IconName::ListTodo,
                            "タスク",
                            rail,
                            !self.calendar_view && self.filters.list.is_none(),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.navigate(Navigation::List(None), window, cx)
                        })),
                    )
                    .child(
                        sidebar_item(
                            t,
                            "sidebar-calendar",
                            IconName::CalendarDays,
                            "時間割",
                            rail,
                            self.calendar_view,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.navigate(Navigation::Calendar, window, cx)
                        })),
                    )
                    .child(div().h(px(16.)).flex_shrink_0())
                    .child(lists)
                    .child(
                        div()
                            .v_flex()
                            .py_2()
                            .flex_shrink_0()
                            .child(
                                sidebar_item(
                                    t,
                                    "sidebar-collections",
                                    IconName::Folders,
                                    "リスト・カレンダー",
                                    rail,
                                    matches!(self.popup, Some(Popup::Collections)),
                                )
                                .disabled(self.info.is_none())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_popup(Popup::Collections, window, cx)
                                })),
                            )
                            .child(
                                sidebar_item(
                                    t,
                                    "sidebar-settings",
                                    IconName::Settings2,
                                    "表示設定",
                                    rail,
                                    matches!(self.popup, Some(Popup::Settings(_))),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_popup(
                                        Popup::Settings(this.settings.clone()),
                                        window,
                                        cx,
                                    )
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
