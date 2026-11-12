use crate::app::*;
use crate::presentation::task_list::subtitle;
use gpui_kit::{
    assets::IconName,
    component::{Icon, Sizable},
};

impl Desktop {
    pub(crate) fn task_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.tokens;
        let count = if self.task_loading {
            "読み込み中…".into()
        } else if self.tasks.loaded {
            format!(
                "{} {}件{}",
                match self.filters.status {
                    StatusFilter::Open => "未完了",
                    StatusFilter::Done => "完了",
                    StatusFilter::Any => "すべて",
                },
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
        let filter_count = usize::from(self.filters.list.is_some())
            + usize::from(self.filters.status != StatusFilter::Open)
            + usize::from(self.filters.due != 0)
            + usize::from(self.filters.blocks != BlockFilter::Any);
        let collection = self
            .filters
            .list
            .or_else(|| self.info.as_ref().map(|info| info.default_list))
            .and_then(|id| {
                self.collections.iter().find_map(|c| match c {
                    Collection::List(list) if list.key.id == id => {
                        Some(list.data.as_str().to_owned())
                    }
                    _ => None,
                })
            });
        let (empty_title, empty_hint) = if self.task_loading || !self.tasks.loaded {
            if self.read_error && !self.task_loading {
                (
                    "データを読み込めませんでした",
                    "画面下の「再試行」から読み直せます。",
                )
            } else {
                ("タスクを読み込んでいます…", "")
            }
        } else if filter_count > 0 || !self.search.read(cx).value().is_empty() {
            (
                "条件に合うタスクがありません",
                "検索語や絞り込みを変更してください。",
            )
        } else {
            (
                "未完了のタスクはありません",
                "次に取り組むことを、上の入力欄から追加できます。",
            )
        };
        let search_input = Styled::h(Input::new(&self.search).small(), px(28.))
            .text_size(px(13.))
            .aria_label("タスク名を検索")
            .prefix(
                Icon::new(IconName::Search)
                    .with_size(px(14.))
                    .text_color(t.text_secondary),
            )
            .cleanable(true)
            .bg(t.surface)
            .bordered(false)
            .focus_bordered(false);
        let quick_input = Styled::h(Input::new(&self.quick).small(), px(32.))
            .text_size(px(14.))
            .bg(t.surface)
            .border_color(t.border)
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
            .size_full()
            .min_w_0()
            .child(
                div()
                    .h_flex()
                    .h(px(40.))
                    .px(px(12.))
                    .flex_shrink_0()
                    .gap(px(8.))
                    .justify_between()
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("タスク"),
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
            .child(
                div()
                    .v_flex()
                    .flex_shrink_0()
                    .gap(px(8.))
                    .px(px(12.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .child(
                        div()
                            .h_flex()
                            .gap(px(6.))
                            .child(
                                input_frame(self.search.entity_id(), &self.search.focus_handle(cx), t)
                                    .flex_1()
                                    .min_w_0()
                                    .child(search_input),
                            )
                            .child(
                                button(t, "filters")
                                    .ghost()
                                    .compact()
                                    .h(px(28.))
                                    .icon(IconName::ListFilter)
                                    .label(if filter_count == 0 {
                                        "絞り込み".into()
                                    } else {
                                        format!("絞り込み {filter_count}")
                                    })
                                    .when(filter_count > 0, |b| {
                                        b.bg(t.selection_background).text_color(t.selection_text)
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_popup(
                                            Popup::Filters(this.filters.clone()),
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap(px(2.))
                            .child(
                                div()
                                    .h_flex()
                                    .gap(px(6.))
                                    .child(
                                        input_frame(
                                            self.quick.entity_id(),
                                            &self.quick.focus_handle(cx),
                                            t,
                                        )
                                        .flex_1()
                                        .min_w_0()
                                        .child(quick_input),
                                    )
                                    .child(
                                        button(t, "add-task")
                                            .ghost()
                                            .compact()
                                            .w(px(32.))
                                            .h(px(32.))
                                            .icon(IconName::Plus)
                                            .accessibility_label("タスクを追加")
                                            .tooltip("タスクを追加 · Enter")
                                            .disabled(
                                                self.busy || self.info.is_none() || self.recovery,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_task(window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .h(px(24.))
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(px(12.))
                                            .text_color(t.text_secondary)
                                            .child("追加先"),
                                    )
                                    .child(
                                        button(t, "collections")
                                            .ghost()
                                            .compact()
                                            .h(px(24.))
                                            .min_w_0()
                                            .max_w(px(220.))
                                            .icon(IconName::List)
                                            .label(
                                                collection
                                                    .clone()
                                                    .unwrap_or_else(|| "リストを管理".into()),
                                            )
                                            .accessibility_label("追加先を確認・リストとカレンダーを管理")
                                            .tooltip(
                                                collection
                                                    .map(|name| {
                                                        format!("追加先: {name} · リスト・カレンダーを管理")
                                                    })
                                                    .unwrap_or_else(|| "リスト・カレンダーを管理".into()),
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_popup(Popup::Collections, window, cx)
                                            })),
                                    ),
                            ),
                    )
                    .when_some(self.quick_validation.clone(), |d, message| {
                        d.child(super::validation_message("quick-error", message, t))
                    }),
            )
            .child(
                div()
                    .id("task-list")
                    .key_context("TaskList")
                    .track_focus(&self.list_focus)
                    .focusable()
                    .focus(move |s| s.shadow(focus_outline(t)))
                    .role(Role::List)
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .px(px(4.))
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
                                .v_flex()
                                .items_start()
                                .gap(px(8.))
                                .px(px(12.))
                                .py(px(24.))
                                .child(div().text_size(px(14.)).child(empty_title))
                                .when(!empty_hint.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(18.))
                                            .text_color(t.text_secondary)
                                            .child(empty_hint),
                                    )
                                })
                                .when(self.tasks.loaded, |d| {
                                    d.child(
                                        button(t, "empty-add-task")
                                            .ghost()
                                            .compact()
                                            .h(px(28.))
                                            .icon(IconName::Plus)
                                            .label("タスクを追加")
                                            .disabled(
                                                self.busy || self.info.is_none() || self.recovery,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.navigate(Navigation::QuickInput, window, cx)
                                            })),
                                    )
                                }),
                        )
                    })
                    .child(task_list),
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
            .min_h(px(t.row_height))
            .w_full()
            .h_flex()
            .gap(px(4.))
            .px(px(6.))
            .py(px(1.))
            .rounded(px(4.))
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
                    .v_flex()
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
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .line_clamp(2)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(if selected {
                                t.selection_text
                            } else {
                                t.text_secondary
                            })
                            .truncate()
                            .child(subtitle),
                    )
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
