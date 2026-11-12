use crate::app::*;
use crate::presentation::task_list::subtitle;
use gpui_kit::{
    assets::IconName,
    component::{
        Icon, Selectable, Sizable,
        menu::{DropdownMenu, PopupMenuItem},
    },
};

impl Desktop {
    pub(crate) fn task_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.tokens;
        let status = match self.filters.status {
            StatusFilter::Open => "未完了",
            StatusFilter::Done => "完了",
            StatusFilter::Any => "すべて",
        };
        let count = if self.task_loading {
            "読み込み中…".into()
        } else if self.tasks.loaded {
            format!(
                "{}件{}",
                self.tasks.total,
                if self.tasks.held_done.is_empty() {
                    String::new()
                } else {
                    format!(" · 今完了 {}", self.tasks.held_done.len())
                }
            )
        } else {
            "未取得".into()
        };
        let weak = cx.weak_entity();
        let task_list = list(self.task_list.clone(), move |index, window, cx| {
            weak.update(cx, |this, cx| this.task_row(index, window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .flex_1()
        .min_h_0();
        let filtered = self.filters.due != 0
            || self.filters.blocks != BlockFilter::Any
            || !self.search.read(cx).value().is_empty();
        let normal_empty = self.tasks.loaded && !self.task_loading && !filtered;
        let (empty_title, empty_hint) = if self.task_loading || !self.tasks.loaded {
            if self.read_error && !self.task_loading {
                (
                    "データを読み込めませんでした",
                    "画面下の「再試行」から読み直せます。",
                )
            } else {
                ("タスクを読み込んでいます…", "")
            }
        } else if filtered {
            (
                "条件に合うタスクがありません",
                "検索語や絞り込みを変更してください。",
            )
        } else {
            (
                match self.filters.status {
                    StatusFilter::Open => "未完了のタスクはありません",
                    StatusFilter::Done => "完了したタスクはありません",
                    StatusFilter::Any => "タスクはまだありません",
                },
                "Ctrl+Nでもタスクを作成できます。",
            )
        };
        div()
            .v_flex()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .h_flex()
                    .h(px(44.))
                    .px(px(12.))
                    .flex_shrink_0()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border)
                    .when(!self.calendar_view, |d| {
                        let mut views = div()
                            .id("task-status-views")
                            .role(Role::Group)
                            .aria_label("タスクの状態")
                            .h_flex()
                            .flex_shrink_0()
                            .gap(px(4.));
                        for (id, status, label) in [
                            ("task-view-open", StatusFilter::Open, "未完了"),
                            ("task-view-done", StatusFilter::Done, "完了"),
                            ("task-view-any", StatusFilter::Any, "すべて"),
                        ] {
                            let selected = self.filters.status == status;
                            views = views.child(
                                button(t, id)
                                    .h(px(28.))
                                    .px(px(10.))
                                    .rounded(px(9999.))
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(510.))
                                    .label(label)
                                    .accessibility_label(label)
                                    .selected(selected)
                                    .toggled(selected)
                                    .tab_stop(true)
                                    .text_color(t.text_secondary)
                                    .when(selected, |b| {
                                        b.bg(t.selection_background).text_color(t.selection_text)
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.apply_filters(
                                            Filters {
                                                status,
                                                ..this.filters.clone()
                                            },
                                            window,
                                            cx,
                                        );
                                    })),
                            );
                        }
                        d.child(views).child(div().flex_1().min_w_0())
                    })
                    .child(self.filter_trigger(window, cx))
                    .when(self.search_open, |d| {
                        let search_input = Styled::h(Input::new(&self.search).small(), px(28.))
                            .text_size(px(13.))
                            .aria_label("タスク名を検索")
                            .prefix(
                                Icon::new(IconName::Search)
                                    .with_size(px(14.))
                                    .text_color(t.text_secondary),
                            )
                            .cleanable(true)
                            .bg(t.canvas)
                            .bordered(false)
                            .focus_bordered(false);
                        d.child(
                            input_frame(self.search.entity_id(), &self.search.focus_handle(cx), t)
                                .flex_1()
                                .min_w_0()
                                .child(search_input),
                        )
                    }),
            )
            .when(!self.tasks.rows.is_empty(), |d| {
                d.child(
                    div()
                        .h_flex()
                        .h(px(36.))
                        .flex_shrink_0()
                        .mx(px(8.))
                        .mt(px(8.))
                        .px(px(8.))
                        .gap(px(8.))
                        .rounded(px(8.))
                        .bg(t.surface)
                        .child(
                            Icon::new(match self.filters.status {
                                StatusFilter::Open => IconName::Circle,
                                StatusFilter::Done => IconName::CircleCheck,
                                StatusFilter::Any => IconName::ListTodo,
                            })
                            .with_size(px(14.))
                            .text_color(t.text_secondary),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(13.))
                                .font_weight(FontWeight(510.))
                                .child(status),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .child(count),
                        ),
                )
            })
            .child(
                div()
                    .id("task-list")
                    .key_context("TaskList")
                    .track_focus(&self.list_focus)
                    .focusable()
                    .border_1()
                    .border_color(transparent_black())
                    .focus_visible(move |s| s.border_color(t.focus_ring))
                    .role(Role::List)
                    .aria_label("タスク一覧")
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .px(px(8.))
                    .on_action(cx.listener(|this, _: &SelectNext, window, cx| {
                        this.select_relative(1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &SelectPrevious, window, cx| {
                        this.select_relative(-1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                        if let Some(id) = this.tasks.visible_selection(this.selected) {
                            this.navigate(Navigation::ListTask(id, false), window, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &CompleteSelected, window, cx| {
                        if let Some(id) = this.tasks.visible_selection(this.selected) {
                            this.toggle_task(id, window, cx);
                        }
                    }))
                    .when(self.tasks.rows.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .v_flex()
                                .items_center()
                                .justify_center()
                                .px(px(12.))
                                .py(px(24.))
                                .child(
                                    div()
                                        .v_flex()
                                        .items_start()
                                        .w_full()
                                        .max_w(px(320.))
                                        .gap(px(12.))
                                        .when(normal_empty, |d| {
                                            d.child(
                                                Icon::new(IconName::ListTodo)
                                                    .with_size(px(28.))
                                                    .text_color(t.text_secondary),
                                            )
                                        })
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .child(empty_title),
                                        )
                                        .when(!empty_hint.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(18.))
                                                    .text_color(t.text_secondary)
                                                    .child(empty_hint),
                                            )
                                        })
                                        .when(normal_empty, |d| {
                                            d.child(
                                                button(t, "empty-add-task")
                                                    .primary()
                                                    .h(px(32.))
                                                    .px(px(12.))
                                                    .icon(IconName::Plus)
                                                    .label("タスクを作成")
                                                    .disabled(
                                                        self.busy
                                                            || self.info.is_none()
                                                            || self.recovery,
                                                    )
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.navigate(
                                                                Navigation::QuickInput,
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                    })
                    .when(!self.tasks.rows.is_empty(), |d| d.child(task_list)),
            )
            .when(self.tasks.earlier || self.tasks.later, |d| {
                d.child(
                    div()
                        .h_flex()
                        .flex_shrink_0()
                        .h(px(32.))
                        .px(px(12.))
                        .gap(px(4.))
                        .justify_end()
                        .border_t_1()
                        .border_color(t.border)
                        .child(
                            button(t, "previous-tasks")
                                .ghost()
                                .compact()
                                .h(px(24.))
                                .icon(IconName::ChevronUp)
                                .label("前へ")
                                .accessibility_label("前のタスクを読み込む")
                                .tooltip("前のタスクを読み込む")
                                .disabled(!self.tasks.earlier || self.task_loading)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.page_tasks(true, window, cx)
                                })),
                        )
                        .child(
                            button(t, "next-tasks")
                                .ghost()
                                .compact()
                                .h(px(24.))
                                .icon(IconName::ChevronDown)
                                .label("次へ")
                                .accessibility_label("次のタスクを読み込む")
                                .tooltip("次のタスクを読み込む")
                                .disabled(!self.tasks.later || self.task_loading)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.page_tasks(false, window, cx)
                                })),
                        ),
                )
            })
            .into_any_element()
    }

    pub(crate) fn quick_task_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.tokens;
        let quick_list = self
            .quick_list
            .or(self.filters.list)
            .or_else(|| self.info.as_ref().map(|info| info.default_list));
        let collection = quick_list.and_then(|id| {
            self.collections.iter().find_map(|c| match c {
                Collection::List(list) if list.key.id == id => Some(list.data.as_str().to_owned()),
                _ => None,
            })
        });
        let quick_input = Styled::h(Input::new(&self.quick), px(40.))
            .text_size(px(18.))
            .font_weight(FontWeight(600.))
            .bg(t.surface)
            .bordered(false)
            .focus_bordered(false)
            .aria_label("追加するタスク名")
            .on_paste({
                let weak = cx.weak_entity();
                move |item, window, cx| {
                    let invalid = item.text().is_some_and(|text| text.contains(['\r', '\n']));
                    if invalid {
                        let _ = weak.update(cx, |this, cx| {
                            let message =
                                "タイトルは1行で入力してください。貼付前の入力を保持しています"
                                    .to_owned();
                            this.quick_validation = Some(message.clone());
                            this.fail(message, cx);
                        });
                    } else {
                        let weak = weak.clone();
                        window.on_next_frame(move |_, cx| {
                            let _ = weak.update(cx, |this, _| this.quick_ready = true);
                        });
                    }
                    invalid
                }
            });
        div()
            .v_flex()
            .w_full()
            .min_w_0()
            .px(px(6.))
            .child(
                div()
                    .h_flex()
                    .min_w_0()
                    .gap(px(6.))
                    .py(px(6.))
                    .px(px(12.))
                    .child(
                        button(t, "quick-list-picker")
                            .ghost()
                            .compact()
                            .h(px(28.))
                            .min_w_0()
                            .max_w(px(320.))
                            .icon(IconName::List)
                            .label(collection.clone().unwrap_or_else(|| "リストを選択".into()))
                            .accessibility_label("タスクの追加先リストを選択")
                            .tooltip(
                                collection
                                    .map(|name| format!("追加先: {name}"))
                                    .unwrap_or_else(|| "追加先リストを選択".into()),
                            )
                            .map(|trigger| {
                                super::menu_trigger("quick-list-focus", trigger, false, window, cx)
                            })
                            .dropdown_menu({
                                let weak = cx.weak_entity();
                                let lists: Vec<_> = self
                                    .collections
                                    .iter()
                                    .filter_map(|collection| match collection {
                                        Collection::List(list) => {
                                            Some((list.key.id, list.data.as_str().to_owned()))
                                        }
                                        _ => None,
                                    })
                                    .collect();
                                move |mut menu, _, _| {
                                    for (id, title) in &lists {
                                        let id = *id;
                                        let weak = weak.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(title.clone())
                                                .icon(IconName::List)
                                                .checked(quick_list == Some(id))
                                                .on_click(move |_, _, cx| {
                                                    let _ = weak.update(cx, |this, cx| {
                                                        this.quick_list = Some(id);
                                                        this.quick_version += 1;
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .with_size(px(14.))
                            .text_color(t.text_secondary),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(13.))
                            .text_color(t.text_secondary)
                            .child("新規タスク"),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .py(px(12.))
                    .pl(px(6.))
                    .pr(px(48.))
                    .child(quick_input),
            )
            .when_some(self.quick_validation.clone(), |d, message| {
                d.child(super::validation_message("quick-error", message, t))
            })
            .child(
                div()
                    .h_flex()
                    .justify_end()
                    .p(px(12.))
                    .border_t_1()
                    .border_color(t.border)
                    .child(
                        button(t, "add-task")
                            .primary()
                            .h(px(32.))
                            .px(px(12.))
                            .label("タスクを作成")
                            .accessibility_label("タスクを作成")
                            .tooltip("タスクを作成 · Enter")
                            .disabled(self.busy || self.info.is_none() || self.recovery)
                            .on_click(cx.listener(|this, _, window, cx| this.add_task(window, cx))),
                    ),
            )
            .into_any_element()
    }

    fn task_row(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.tasks.rows.get(index) else {
            return div().into_any_element();
        };
        self.rendered_rows += 1;
        self.diagnostics.sample("task_row_built", 0, 0, 1);
        let t = self.tokens;
        let id = row.task.key.id;
        let title = row.task.data.title.as_str().to_owned();
        let done = row.task.data.status == TaskStatus::Done;
        let selected = self.selected == Some(id);
        let summary = TaskSummary {
            key: row.task.key,
            title: row.task.data.title.clone(),
            status: row.task.data.status,
        };
        let subtitle = subtitle(row, &self.options.zone);
        let row_id = SharedString::from(format!("task-{id}"));
        div()
            .id(row_id)
            .h(px(t.row_height))
            .w_full()
            .min_w_0()
            .h_flex()
            .gap(px(8.))
            .px(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(transparent_black())
            .bg(if selected {
                t.selection_background
            } else {
                t.canvas
            })
            .text_color(if selected {
                t.selection_text
            } else if done {
                t.text_secondary
            } else {
                t.text_primary
            })
            .hover(move |s| {
                s.bg(if selected {
                    t.selection_background
                } else {
                    t.surface_hover
                })
                .border_color(if t.canvas == t.surface_hover {
                    t.focus_ring
                } else {
                    transparent_black()
                })
            })
            .child(
                button(t, SharedString::from(format!("done-{id}")))
                    .ghost()
                    .compact()
                    .icon(if done {
                        IconName::CircleCheck
                    } else {
                        IconName::Circle
                    })
                    .accessibility_label(format!(
                        "{} {}",
                        title,
                        if done {
                            "未完了に戻す"
                        } else {
                            "完了にする"
                        }
                    ))
                    .tooltip(if done {
                        "未完了に戻す"
                    } else {
                        "完了にする"
                    })
                    .w(px(28.))
                    .h(px(28.))
                    .flex_shrink_0()
                    .when(selected, |b| b.text_color(t.selection_text))
                    .disabled(self.busy || self.recovery)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.toggle_task(id, window, cx);
                    })),
            )
            .child(
                div()
                    .id(SharedString::from(format!("title-{id}")))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .h_flex()
                    .gap(px(12.))
                    .cursor_pointer()
                    .role(Role::ListItem)
                    .aria_label(format!(
                        "タスク {} {} {}",
                        title,
                        if done { "完了" } else { "未完了" },
                        subtitle
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .font_weight(FontWeight(510.))
                            .line_height(px(20.))
                            .truncate()
                            .child(title),
                    )
                    .when(!self.calendar_view, |d| {
                        d.child(
                            div()
                                .min_w_0()
                                .max_w(px(320.))
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(if selected {
                                    t.selection_text
                                } else {
                                    t.text_secondary
                                })
                                .truncate()
                                .child(subtitle),
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            if this.tasks.visible_selection(Some(id)).is_none() {
                                return;
                            }
                            this.selected = Some(id);
                            this.list_focus.focus(window, cx);
                            this.start_drag(
                                DragSource::Task(summary.clone()),
                                event.position,
                                window,
                                cx,
                            );
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !this.suppress_click
                            && !this.drag.as_ref().is_some_and(|drag| drag.active)
                        {
                            this.navigate(Navigation::ListTask(id, false), window, cx);
                        }
                    })),
            )
            .child(
                button(t, SharedString::from(format!("time-{id}")))
                    .ghost()
                    .compact()
                    .w(px(28.))
                    .h(px(28.))
                    .flex_shrink_0()
                    .icon(IconName::Clock)
                    .when(selected, |b| b.text_color(t.selection_text))
                    .accessibility_label("作業時間を指定・編集")
                    .tooltip("作業時間を指定・編集")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.navigate(Navigation::ListTask(id, true), window, cx);
                    })),
            )
            .into_any_element()
    }
}
