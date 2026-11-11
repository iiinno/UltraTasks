use crate::app::*;
use crate::presentation::task_list::subtitle;

impl Desktop {
    pub(crate) fn task_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.tokens;
        let count = if self.tasks.loaded {
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
            "読み込み中".into()
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
        let empty_message = if !self.tasks.loaded {
            "タスクを読み込んでいます…"
        } else if filter_count > 0 || !self.search.read(cx).value().is_empty() {
            "条件に合うタスクがありません。検索語や絞り込みを変更できます"
        } else {
            "未完了のタスクはありません。上の入力欄から追加できます"
        };
        div()
            .v_flex()
            .gap_2()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .h_flex()
                    .h(px(40.))
                    .flex_shrink_0()
                    .gap_2()
                    .justify_between()
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child("タスク"))
                    .child(div().text_size(px(12.)).text_color(t.text_secondary).child(count)),
            )
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        input_frame(self.search.entity_id(), &self.search.focus_handle(cx), t)
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&self.search)
                                    .aria_label("タスク名を検索")
                                    .cleanable(true)
                                    .bg(t.surface)
                                    .border_color(t.border)
                                    .focus_bordered(false),
                            ),
                    )
                    .child(
                        button(t, "filters")
                            .label(if filter_count == 0 { "絞り込み".into() } else { format!("絞り込み {filter_count}") })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_popup(Popup::Filters(this.filters.clone()), window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .justify_between()
                    .mt_2()
                    .child(div().flex_shrink_0().child("タスクを追加"))
                    .child(
                        button(t, "collections")
                            .flex_1()
                            .min_w_0()
                            .label(collection.as_ref().map(|name| format!("追加先: {name}")).unwrap_or_else(|| "リスト・カレンダー".into()))
                            .accessibility_label("リスト・カレンダーを管理")
                            .tooltip(collection.map(|name| format!("追加先: {name} · リスト・カレンダーを管理")).unwrap_or_else(|| "リスト・カレンダーを管理".into()))
                            .on_click(cx.listener(|this, _, window, cx| this.open_popup(Popup::Collections, window, cx))),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        input_frame(self.quick.entity_id(), &self.quick.focus_handle(cx), t)
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&self.quick)
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
                                                    let message = "タイトルは1行で入力してください。貼付前の入力を保持しています".to_owned();
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
                                    }),
                            ),
                    )
                    .child(
                        button(t, "add-task")
                            .primary()
                            .label("追加")
                            .disabled(self.busy || self.info.is_none() || self.recovery)
                            .on_click(cx.listener(|this, _, window, cx| this.add_task(window, cx))),
                    ),
            )
            .when_some(self.quick_validation.clone(), |d, message| {
                d.child(super::validation_message("quick-error", message, t))
            })
            .child(
                div().text_size(px(12.)).text_color(t.text_secondary).child(if self.task_loading {
                    "検索中… 入力は続けられます"
                } else {
                    "Enterで追加 · 時間から作業枠を指定"
                }),
            )
            .child(
                div()
                    .id("task-list")
                    .key_context("TaskList")
                    .track_focus(&self.list_focus)
                    .focusable()
                    .focus(move |s| s.shadow(focus_outline(t)))
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .on_action(cx.listener(|this, _: &SelectNext, window, cx| this.select_relative(1, window, cx)))
                    .on_action(cx.listener(|this, _: &SelectPrevious, window, cx| this.select_relative(-1, window, cx)))
                    .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                        if let Some(id) = this.selected {
                            this.navigate(Navigation::Task(id), window, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &CompleteSelected, window, cx| {
                        if let Some(id) = this.selected {
                            this.toggle_task(id, window, cx);
                        }
                    }))
                    .when(self.tasks.rows.is_empty(), |d| {
                        d.child(div().p_4().text_color(t.text_secondary).child(empty_message))
                    })
                    .child(task_list),
            )
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .child(
                        button(t, "previous-tasks")
                            .label("前のタスク")
                            .disabled(!self.tasks.earlier || self.task_loading)
                            .on_click(cx.listener(|this, _, window, cx| this.page_tasks(true, window, cx))),
                    )
                    .child(
                        button(t, "next-tasks")
                            .label("次のタスク")
                            .disabled(!self.tasks.later || self.task_loading)
                            .on_click(cx.listener(|this, _, window, cx| this.page_tasks(false, window, cx))),
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
        let long = title.chars().count() > 24;
        let summary = TaskSummary {
            key: row.task.key,
            title: row.task.data.title.clone(),
            status: row.task.data.status,
        };
        let subtitle = subtitle(row, &self.options.zone);
        let row_id = SharedString::from(format!("task-{id}"));
        div()
            .id(row_id)
            .h(px(if long {
                t.long_row_height
            } else {
                t.row_height
            }))
            .w_full()
            .h_flex()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(t.border)
            .bg(if selected {
                t.selection_background
            } else {
                t.canvas
            })
            .text_color(if selected {
                t.selection_text
            } else {
                t.text_primary
            })
            .hover(move |s| {
                s.bg(if selected {
                    t.selection_background
                } else {
                    t.surface_hover
                })
                .border_color(t.focus_ring)
            })
            .child(
                button(t, SharedString::from(format!("done-{id}")))
                    .label(if done { "✓" } else { "○" })
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
                    .w(px(32.))
                    .h(px(32.))
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
                    .gap_1()
                    .cursor_pointer()
                    .role(Role::ListItem)
                    .aria_label(format!(
                        "タスク {} {} {}",
                        title,
                        if done { "完了" } else { "未完了" },
                        subtitle
                    ))
                    .child(div().line_height(px(20.)).line_clamp(2).child(title))
                    .child(
                        div()
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
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
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
                            this.navigate(Navigation::Task(id), window, cx);
                        }
                    })),
            )
            .child(
                button(t, SharedString::from(format!("time-{id}")))
                    .label("時間")
                    .accessibility_label("作業時間を指定・編集")
                    .tooltip("作業時間を指定・編集")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.navigate(Navigation::Block(id), window, cx);
                    })),
            )
            .into_any_element()
    }
}
