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
        .px_2()
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
                .gap_2()
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
            .gap_1()
            .overflow_y_scroll()
            .when(!rail, |d| {
                d.child(
                    div()
                        .h(px(28.))
                        .h_flex()
                        .px_2()
                        .text_size(px(12.))
                        .text_color(t.text_secondary)
                        .child("リスト"),
                )
            })
            .child(
                sidebar_item(
                    t,
                    "all-lists",
                    IconName::ListTodo,
                    "すべてのリスト",
                    rail,
                    self.filters.list.is_none(),
                )
                .toggled(self.filters.list.is_none())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.navigate(Navigation::List(None), window, cx)
                })),
            );
        for collection in &self.collections {
            let Collection::List(list) = collection else {
                continue;
            };
            let id = list.key.id;
            lists = lists.child(
                sidebar_item(
                    t,
                    SharedString::from(format!("sidebar-list-{id}")),
                    IconName::List,
                    list.data.as_str().to_owned(),
                    rail,
                    self.filters.list == Some(id),
                )
                .toggled(self.filters.list == Some(id))
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
                            .h(px(28.))
                            .w(px(28.))
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
                            .h(px(28.))
                            .w(px(28.))
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
            .aria_label("プランナーとリスト")
            .v_flex()
            .w(px(if rail { 52. } else { 192. }))
            .h_full()
            .min_h_0()
            .flex_shrink_0()
            .border_r_1()
            .border_color(t.border)
            .bg(t.surface)
            .text_color(t.text_secondary)
            .child(
                div()
                    .h_flex()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px_4()
                    .text_color(t.text_primary)
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(rail, |d| d.px_0().justify_center())
                    .child(if rail { "UT" } else { "UltraTasks" }),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .gap_1()
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .mb_2()
                            .when(rail, |d| d.flex_col())
                            .child(
                                button(t, "sidebar-search")
                                    .icon(IconName::Search)
                                    .tooltip("タスクを検索 Ctrl+K")
                                    .accessibility_label("タスクを検索")
                                    .when(rail, |button| button.w_full())
                                    .when(!rail, |button| button.flex_1().label("検索"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.navigate(Navigation::SearchInput, window, cx)
                                    })),
                            )
                            .child(
                                button(t, "sidebar-new-task")
                                    .icon(IconName::SquarePen)
                                    .w(px(32.))
                                    .when(rail, |button| button.w_full())
                                    .tooltip("タスクを追加 Ctrl+N")
                                    .accessibility_label("タスクを追加")
                                    .disabled(self.info.is_none() || self.recovery)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.navigate(Navigation::QuickInput, window, cx)
                                    })),
                            ),
                    )
                    .child(
                        sidebar_item(
                            t,
                            "sidebar-planner",
                            IconName::CalendarDays,
                            "プランナー",
                            rail,
                            false,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.navigate(Navigation::Tasks, window, cx)
                        })),
                    )
                    .child(div().h(px(12.)).flex_shrink_0())
                    .child(lists)
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
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
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.open_popup(Popup::Collections, window, cx)
                                    },
                                )),
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
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.open_popup(
                                            Popup::Settings(this.settings.clone()),
                                            window,
                                            cx,
                                        )
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }
}
