use crate::app::*;
use crate::diagnostics::micros;
use gpui_kit::{
    assets::IconName,
    component::{Icon, Selectable, Sizable, radio::RadioGroup, tooltip::Tooltip},
};
use std::time::Instant as Clock;

fn operation_name(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::TaskCreated => "タスクの追加",
        OperationKind::TaskEdited => "タスクの編集",
        OperationKind::TaskCompleted => "タスクの完了",
        OperationKind::TaskReopened => "タスクを未完了に戻す操作",
        OperationKind::TaskDeleted => "タスクの削除",
        OperationKind::EventCreated => "予定の追加",
        OperationKind::EventEdited => "予定の編集",
        OperationKind::EventDeleted => "予定の削除",
        OperationKind::BlockCreated => "作業時間の追加",
        OperationKind::BlockEdited => "作業時間の編集",
        OperationKind::BlockDeleted => "作業時間の削除",
        OperationKind::CollectionCreated => "リスト・カレンダーの追加",
        OperationKind::CollectionEdited => "リスト・カレンダーの編集",
        OperationKind::CollectionDeleted => "リスト・カレンダーの削除",
    }
}

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Clock::now();
        self.rendered_rows = 0;
        if let Some(frame) = self.frame.take() {
            let diagnostics = self.diagnostics.clone();
            window.on_next_frame(move |_, _| {
                frame.set(true);
                diagnostics.sample("first_frame_callback", 0, 0, 1);
            });
        }
        let t = self.tokens;
        let width = f32::from(window.viewport_size().width);
        let rail = width < 1000.;
        let compact = width < 780.;
        let mut columns = div().h_flex().items_stretch().flex_1().min_w_0().min_h_0();
        if !compact || !self.compact_agenda {
            let left = if self.form.is_some() {
                self.editor_panel(window, cx)
            } else {
                self.task_panel(window, cx)
            };
            columns = columns.child(
                div()
                    .v_flex()
                    .w(px(360.))
                    .flex_shrink_0()
                    .min_w_0()
                    .min_h_0()
                    .when(compact, |d| d.w_full())
                    .when(!compact, |d| d.border_r_1().border_color(t.border))
                    .child(left),
            );
        }
        if !compact || self.compact_agenda {
            columns = columns.child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.agenda_panel(window, cx)),
            );
        }
        let sidebar = self.sidebar(rail, cx);
        let toolbar = self.location_bar(width, cx);
        let status = self.status_bar(width, cx);
        let popup = self.render_popup(window, cx);
        let root = div()
            .id("ultratasks")
            .key_context("UltraTasks")
            .track_focus(&self.focus)
            .size_full()
            .v_flex()
            .relative()
            .bg(t.canvas)
            .text_color(t.text_primary)
            .font_family("Yu Gothic UI")
            .text_size(px(t.body_size))
            .line_height(px(t.body_line))
            .on_action(cx.listener(|this, _: &NewTask, window, cx| {
                this.navigate(Navigation::QuickInput, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FindTask, window, cx| {
                this.navigate(Navigation::SearchInput, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NewEvent, window, cx| {
                this.navigate(Navigation::Event, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &PreviousDay, window, cx| this.change_day(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextDay, window, cx| this.change_day(1, window, cx)))
            .on_action(cx.listener(|this, _: &Today, window, cx| {
                if let Ok(day) = Date::today(&this.options.zone) {
                    this.navigate(Navigation::Day(day), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PickDate, window, cx| {
                this.open_popup(Popup::Date, window, cx)
            }))
            .on_action(cx.listener(|this, _: &EditSelected, window, cx| {
                if let Some(form) = &this.form {
                    if matches!(form.draft.target, Target::Block { .. }) {
                        form.fields[1].update(cx, |s, cx| s.focus(window, cx));
                    } else {
                        form.title_input.update(cx, |s, cx| s.focus(window, cx));
                    }
                } else if let Some(id) = this.tasks.visible_selection(this.selected) {
                    this.focus_editor_input = true;
                    this.navigate(Navigation::ListTask(id, false), window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &SaveEditor, window, cx| this.save_from_keyboard(window, cx)),
            )
            .on_key_up(cx.listener(|this, _, _, _| {
                this.quick_ready = true;
                this.date_ready = true;
                if let Some(form) = &mut this.form {
                    form.submit_ready = true;
                }
            }))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel(window, cx)))
            .on_action(
                cx.listener(|this, _: &UndoApp, window, cx| this.history_action(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &RedoApp, window, cx| this.history_action(true, window, cx)),
            )
            .on_mouse_move(cx.listener(|this, event, window, cx| this.drag_move(event, window, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_drag(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.drag.take().is_some() {
                        this.restore_focus(window, cx);
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .h_flex()
                    .items_stretch()
                    .size_full()
                    .min_h_0()
                    .child(sidebar)
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(toolbar)
                            .when(compact, |d| {
                                d.child(
                                    div()
                                        .h_flex()
                                        .h(px(36.))
                                        .flex_shrink_0()
                                        .px_3()
                                        .gap_1()
                                        .border_b_1()
                                        .border_color(t.border)
                                        .child(
                                            button(t, "task-tab")
                                                .h(px(28.))
                                                .icon(IconName::ListTodo)
                                                .label(if self.form.is_some() {
                                                    "編集"
                                                } else {
                                                    "タスク"
                                                })
                                                .selected(!self.compact_agenda)
                                                .when(!self.compact_agenda, |button| {
                                                    button
                                                        .bg(t.selection_background)
                                                        .text_color(t.selection_text)
                                                })
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.compact_agenda = false;
                                                    this.focus.focus(window, cx);
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            button(t, "agenda-tab")
                                                .h(px(28.))
                                                .icon(IconName::CalendarDays)
                                                .label("時間割")
                                                .selected(self.compact_agenda)
                                                .when(self.compact_agenda, |button| {
                                                    button
                                                        .bg(t.selection_background)
                                                        .text_color(t.selection_text)
                                                })
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.compact_agenda = true;
                                                    this.focus.focus(window, cx);
                                                    cx.notify();
                                                })),
                                        ),
                                )
                            })
                            .child(columns)
                            .child(status),
                    ),
            )
            .when_some(popup, |d, popup| d.child(popup));
        self.diagnostics.sample(
            "render_callback_cpu",
            0,
            micros(started.elapsed()),
            self.tasks.rows.len() as u64,
        );
        root
    }
}
impl Desktop {
    fn location_bar(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let t = self.tokens;
        let location = self
            .filters
            .list
            .and_then(|id| {
                self.collections
                    .iter()
                    .find_map(|collection| match collection {
                        Collection::List(list) if list.key.id == id => {
                            Some(list.data.as_str().to_owned())
                        }
                        _ => None,
                    })
            })
            .unwrap_or_else(|| {
                if self.filters.list.is_some() {
                    "選択中のリスト".into()
                } else {
                    "プランナー".into()
                }
            });
        div()
            .h_flex()
            .h(px(48.))
            .flex_shrink_0()
            .px_3()
            .gap_1()
            .border_b_1()
            .border_color(t.border)
            .bg(t.surface)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_ellipsis()
                    .child(location),
            )
            .child(
                button(t, "previous-day")
                    .icon(IconName::ChevronLeft)
                    .h(px(28.))
                    .w(px(28.))
                    .accessibility_label("前日")
                    .tooltip("前日 Alt+PageUp")
                    .on_click(cx.listener(|this, _, window, cx| this.change_day(-1, window, cx))),
            )
            .child(
                button(t, "selected-date")
                    .h(px(28.))
                    .label(if width >= 1100. {
                        format!(
                            "{}年 {}",
                            self.options.day.ymd().0,
                            time::date_heading(self.options.day)
                        )
                    } else {
                        time::date_heading(self.options.day)
                    })
                    .accessibility_label(format!(
                        "{} {} 日付を移動",
                        self.options.day.iso(),
                        time::date_heading(self.options.day)
                    ))
                    .tooltip("日付を移動 Ctrl+D")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_popup(Popup::Date, window, cx)),
                    ),
            )
            .child(
                button(t, "next-day")
                    .icon(IconName::ChevronRight)
                    .h(px(28.))
                    .w(px(28.))
                    .accessibility_label("翌日")
                    .tooltip("翌日 Alt+PageDown")
                    .on_click(cx.listener(|this, _, window, cx| this.change_day(1, window, cx))),
            )
            .child(
                button(t, "today")
                    .h(px(28.))
                    .label("今日")
                    .tooltip("今日へ戻る Alt+Home")
                    .disabled(Date::today(&self.options.zone).ok() == Some(self.options.day))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Ok(day) = Date::today(&this.options.zone) {
                            this.navigate(Navigation::Day(day), window, cx);
                        }
                    })),
            )
            .child(div().mx_2().h(px(16.)).w(px(1.)).bg(t.border))
            .child(
                button(t, "add-event")
                    .icon(IconName::CalendarPlus)
                    .h(px(28.))
                    .min_w(px(28.))
                    .when(width >= 900., |button| button.label("予定"))
                    .accessibility_label("予定を追加")
                    .tooltip("予定を追加 Ctrl+Shift+E")
                    .disabled(self.info.is_none() || self.recovery)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.navigate(Navigation::Event, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn status_bar(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let t = self.tokens;
        let status = self.status.clone();
        div()
            .h_flex()
            .min_h(px(32.))
            .flex_shrink_0()
            .px_3()
            .py_1()
            .gap_2()
            .border_t_1()
            .border_color(t.border)
            .bg(t.surface)
            .when(self.error, |d| {
                d.child(
                    Icon::new(IconName::CircleAlert)
                        .with_size(px(14.))
                        .text_color(t.error_text),
                )
            })
            .child(
                div()
                    .id("save-status")
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(if self.error {
                        t.error_text
                    } else {
                        t.text_secondary
                    })
                    .when(self.error, |d| d.bg(t.error_background))
                    .role(Role::Status)
                    .aria_label(status.clone())
                    .line_clamp(if self.error { 2 } else { 1 })
                    .tooltip(move |window, cx| Tooltip::new(status.clone()).build(window, cx))
                    .child(self.status.clone()),
            )
            .when_some(self.saved_task, |d, id| {
                d.child(
                    button(t, "open-saved-task")
                        .h(px(24.))
                        .icon(IconName::ArrowUpRight)
                        .when(width >= 900., |button| button.label("タスクを開く"))
                        .accessibility_label("保存したタスクを開く")
                        .tooltip("保存したタスクを開く")
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.navigate(Navigation::Task(id), window, cx)
                        })),
                )
            })
            .when(
                self.recovery || self.retryable_write() || self.read_error,
                |d| {
                    d.child(
                        button(t, "retry")
                            .h(px(24.))
                            .icon(IconName::RefreshCw)
                            .label(if self.recovery {
                                "保存先を開き直す"
                            } else {
                                "再試行"
                            })
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.retry(window, cx))),
                    )
                },
            )
            .child(
                button(t, "undo")
                    .icon(IconName::Undo2)
                    .h(px(24.))
                    .w(px(28.))
                    .accessibility_label("元に戻す")
                    .tooltip(
                        self.history
                            .undo
                            .as_ref()
                            .map(|e| format!("{}を元に戻す Ctrl+Z", operation_name(e.kind)))
                            .unwrap_or_else(|| "戻せる操作はありません".into()),
                    )
                    .disabled(self.history.undo.is_none() || self.busy || self.recovery)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.history_action(false, window, cx)),
                    ),
            )
            .child(
                button(t, "redo")
                    .icon(IconName::Redo2)
                    .h(px(24.))
                    .w(px(28.))
                    .accessibility_label("やり直す")
                    .tooltip(
                        self.history
                            .redo
                            .as_ref()
                            .map(|e| format!("{}をやり直す Ctrl+Shift+Z", operation_name(e.kind)))
                            .unwrap_or_else(|| "やり直せる操作はありません".into()),
                    )
                    .disabled(self.history.redo.is_none() || self.busy || self.recovery)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.history_action(true, window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popup = self.popup.clone()?;
        let dialog_title = match &popup {
            Popup::Filters(_) => "タスクの絞り込み",
            Popup::Settings(_) => "表示設定",
            Popup::Date => "日付を移動",
            Popup::Collections => "リスト・カレンダー",
            Popup::Leave(_) if self.busy => "保存処理が続いています",
            Popup::Leave(_) => "未保存の入力があります",
            Popup::Delete(_) => "削除しますか",
        };
        let t = self.tokens;
        let mut content = div()
            .id("popup")
            .v_flex()
            .gap_3()
            .w(px(340.))
            .max_h(px(
                (f32::from(window.viewport_size().height) - 80.).clamp(160., 600.)
            ))
            .overflow_y_scroll()
            .p_4()
            .rounded(px(10.))
            .bg(t.surface)
            .when(t.canvas == t.surface_hover, |d| {
                d.border_1().border_color(t.border)
            })
            .when(t.canvas != t.surface_hover, |d| d.shadow_lg())
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(dialog_title),
            );
        match popup {
            Popup::Filters(filters) => {
                content = content
                    .child(
                        div()
                            .id("filter-status-field")
                            .role(Role::Group)
                            .aria_label("状態")
                            .v_flex()
                            .gap_2()
                            .child("状態")
                            .child(
                                RadioGroup::horizontal("filter-status")
                                    .children(["未完了", "完了", "すべて"])
                                    .selected_index(Some(match filters.status {
                                        StatusFilter::Open => 0,
                                        StatusFilter::Done => 1,
                                        StatusFilter::Any => 2,
                                    }))
                                    .on_change(cx.listener(|this, index, _, cx| {
                                        if let Some(Popup::Filters(f)) = &mut this.popup {
                                            f.status = match index {
                                                0 => StatusFilter::Open,
                                                1 => StatusFilter::Done,
                                                _ => StatusFilter::Any,
                                            };
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("filter-due-field")
                            .role(Role::Group)
                            .aria_label("期限")
                            .v_flex()
                            .gap_2()
                            .child("期限")
                            .child(
                                RadioGroup::horizontal("filter-due")
                                    .children(["すべて", "選択日まで", "期限なし"])
                                    .selected_index(Some(filters.due as usize))
                                    .on_change(cx.listener(|this, index, _, cx| {
                                        if let Some(Popup::Filters(f)) = &mut this.popup {
                                            f.due = *index as u8;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("filter-block-field")
                            .role(Role::Group)
                            .aria_label("選択日の作業枠")
                            .v_flex()
                            .gap_2()
                            .child("選択日の作業枠")
                            .child(
                                RadioGroup::horizontal("filter-block")
                                    .children(["すべて", "あり", "なし"])
                                    .selected_index(Some(match filters.blocks {
                                        BlockFilter::Any => 0,
                                        BlockFilter::InSelectedWindow => 1,
                                        BlockFilter::NotInSelectedWindow => 2,
                                    }))
                                    .on_change(cx.listener(|this, index, _, cx| {
                                        if let Some(Popup::Filters(f)) = &mut this.popup {
                                            f.blocks = match index {
                                                0 => BlockFilter::Any,
                                                1 => BlockFilter::InSelectedWindow,
                                                _ => BlockFilter::NotInSelectedWindow,
                                            };
                                        }
                                        cx.notify();
                                    })),
                            ),
                    );
                let lists: Vec<_> = std::iter::once((None, "すべてのリスト".to_owned()))
                    .chain(
                        self.collections
                            .iter()
                            .filter_map(|collection| match collection {
                                Collection::List(list) => {
                                    Some((Some(list.key.id), list.data.as_str().to_owned()))
                                }
                                _ => None,
                            }),
                    )
                    .collect();
                let selected = lists.iter().position(|(id, _)| *id == filters.list);
                content = content.child(
                    div()
                        .id("filter-list-field")
                        .role(Role::Group)
                        .aria_label("リスト")
                        .v_flex()
                        .gap_2()
                        .child("リスト")
                        .child(
                            RadioGroup::vertical("filter-list")
                                .children(lists.iter().map(|(_, title)| title.clone()))
                                .selected_index(selected)
                                .on_change(cx.listener(move |this, index, _, cx| {
                                    if let Some(Popup::Filters(f)) = &mut this.popup
                                        && let Some((id, _)) = lists.get(*index)
                                    {
                                        f.list = *id;
                                    }
                                    cx.notify();
                                })),
                        ),
                );
                content =
                    content.child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(button(t, "reset-filters").label("リセット").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.popup = None;
                                    this.apply_filters(Filters::default(), window, cx);
                                    this.restore_focus(window, cx);
                                }),
                            ))
                            .child(button(t, "apply-filters").primary().label("適用").on_click(
                                cx.listener(|this, _, window, cx| {
                                    if let Some(Popup::Filters(filters)) = this.popup.take() {
                                        this.apply_filters(filters, window, cx);
                                        this.restore_focus(window, cx);
                                    }
                                }),
                            )),
                    );
            }
            Popup::Settings(settings) => {
                content = content
                    .child(
                        div()
                            .id("theme-field")
                            .role(Role::Group)
                            .aria_label("テーマ")
                            .v_flex()
                            .gap_3()
                            .child("テーマ")
                            .child(
                                RadioGroup::horizontal("theme")
                                    .children(["システム", "ライト", "ダーク"])
                                    .selected_index(Some(match settings.theme {
                                        ThemeMode::System => 0,
                                        ThemeMode::Light => 1,
                                        ThemeMode::Dark => 2,
                                    }))
                                    .on_change(cx.listener(|this, index, _, cx| {
                                        if let Some(Popup::Settings(s)) = &mut this.popup {
                                            s.theme = match index {
                                                0 => ThemeMode::System,
                                                1 => ThemeMode::Light,
                                                _ => ThemeMode::Dark,
                                            };
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("motion-field")
                            .role(Role::Group)
                            .aria_label("動き")
                            .v_flex()
                            .gap_3()
                            .pt_2()
                            .child("動き")
                            .child(
                                RadioGroup::horizontal("motion")
                                    .children(["システム", "減らす", "なし"])
                                    .selected_index(Some(match settings.motion {
                                        MotionMode::System => 0,
                                        MotionMode::Reduced => 1,
                                        MotionMode::None => 2,
                                    }))
                                    .on_change(cx.listener(|this, index, _, cx| {
                                        if let Some(Popup::Settings(s)) = &mut this.popup {
                                            s.motion = match index {
                                                0 => MotionMode::System,
                                                1 => MotionMode::Reduced,
                                                _ => MotionMode::None,
                                            };
                                        }
                                        cx.notify();
                                    })),
                            ),
                    );
                content = content
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_secondary)
                            .child("OSの動き軽減・高コントラストを優先します"),
                    )
                    .child(
                        button(t, "apply-settings")
                            .primary()
                            .label("適用して保存")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(Popup::Settings(settings)) = this.popup.clone() {
                                    this.apply_settings(settings, window, cx);
                                }
                            })),
                    );
            }
            Popup::Date => {
                content = content
                    .key_context("DatePicker")
                    .on_action(cx.listener(|this, _: &PickerLeft, window, cx| {
                        this.shift_picker_day(-1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PickerRight, window, cx| {
                        this.shift_picker_day(1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PickerUp, window, cx| {
                        this.shift_picker_day(-7, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PickerDown, window, cx| {
                        this.shift_picker_day(7, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PickerPreviousMonth, window, cx| {
                        this.shift_picker_month(-1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PickerNextMonth, window, cx| {
                        this.shift_picker_month(1, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PickerApply, window, cx| {
                        this.apply_picker(window, cx)
                    }));
                content = content
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_secondary)
                            .child("日付（YYYY-MM-DD）"),
                    )
                    .child(
                        input_frame(
                            self.date_input.entity_id(),
                            &self.date_input.focus_handle(cx),
                            t,
                        )
                        .child(
                            Styled::h(Input::new(&self.date_input).small(), px(32.))
                                .aria_label("移動する日付")
                                .bg(t.surface)
                                .border_color(t.border)
                                .focus_bordered(false),
                        ),
                    )
                    .when_some(self.date_validation.clone(), |d, message| {
                        d.child(super::validation_message("date-error", message, t))
                    })
                    .child(button(t, "apply-date").primary().label("移動").on_click(
                        cx.listener(|this, _, window, cx| this.apply_picker(window, cx)),
                    ));
                use chrono::Datelike;
                let (year, month, _) = Date::parse(&self.date_input.read(cx).value())
                    .unwrap_or(self.options.day)
                    .ymd();
                content = content.child(
                    div()
                        .h_flex()
                        .justify_between()
                        .child(
                            button(t, "previous-month")
                                .icon(IconName::ChevronLeft)
                                .accessibility_label("前月")
                                .tooltip("前月 PageUp")
                                .w(px(28.))
                                .h(px(28.))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.shift_picker_month(-1, window, cx)
                                })),
                        )
                        .child(format!("{year}年 {month}月"))
                        .child(
                            button(t, "next-month")
                                .icon(IconName::ChevronRight)
                                .accessibility_label("翌月")
                                .tooltip("翌月 PageDown")
                                .w(px(28.))
                                .h(px(28.))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.shift_picker_month(1, window, cx)
                                })),
                        ),
                );
                let first_weekday = chrono::NaiveDate::from_ymd_opt(year, month as u32, 1)
                    .map(|date| date.weekday().num_days_from_monday() as i32)
                    .unwrap_or(0);
                content = content.child(
                    div().h_flex().gap_1().children(
                        ["月", "火", "水", "木", "金", "土", "日"]
                            .into_iter()
                            .map(|label| {
                                div()
                                    .w(px(36.))
                                    .text_center()
                                    .text_color(t.text_secondary)
                                    .child(label)
                            }),
                    ),
                );
                for week in 0..6 {
                    let mut row = div().h_flex().gap_1();
                    for weekday in 0..7 {
                        let number = week * 7 + weekday + 1 - first_weekday;
                        if number > 0
                            && let Ok(day) =
                                Date::parse(&format!("{year:04}-{month:02}-{number:02}"))
                        {
                            row = row.child(
                                button(t, ("calendar-day", number as u32))
                                    .label(number.to_string())
                                    .accessibility_label(format!(
                                        "{} {}",
                                        day.iso(),
                                        time::date_heading(day)
                                    ))
                                    .when(
                                        Date::parse(&self.date_input.read(cx).value()).ok()
                                            == Some(day),
                                        |button| {
                                            button
                                                .selected(true)
                                                .bg(t.selection_background)
                                                .text_color(t.selection_text)
                                        },
                                    )
                                    .w(px(36.))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.date_validation = None;
                                        this.navigate(Navigation::Day(day), window, cx)
                                    })),
                            );
                            continue;
                        }
                        row = row.child(div().w(px(36.)).h(px(32.)));
                    }
                    content = content.child(row);
                }
            }
            Popup::Collections => {
                for (index, collection) in self.collections.iter().enumerate() {
                    let collection = collection.clone();
                    let (icon, title) = match &collection {
                        Collection::List(list) => {
                            (IconName::List, format!("リスト · {}", list.data.as_str()))
                        }
                        Collection::Calendar(cal) => (
                            IconName::Calendar,
                            format!("カレンダー · {}", cal.data.as_str()),
                        ),
                    };
                    content = content.child(
                        button(t, ("collection", index))
                            .icon(icon)
                            .tooltip(title.clone())
                            .label(title)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.navigate(
                                    Navigation::Entity(match &collection {
                                        Collection::List(list) => core::EntityId::List(list.key.id),
                                        Collection::Calendar(cal) => {
                                            core::EntityId::Calendar(cal.key.id)
                                        }
                                    }),
                                    window,
                                    cx,
                                )
                            })),
                    );
                }
                content = content
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                button(t, "new-list")
                                    .icon(IconName::Plus)
                                    .label("リスト")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.navigate(Navigation::Collection(false), window, cx)
                                    })),
                            )
                            .child(
                                button(t, "new-calendar")
                                    .icon(IconName::Plus)
                                    .label("カレンダー")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.navigate(Navigation::Collection(true), window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                button(t, "collections-before")
                                    .icon(IconName::ChevronLeft)
                                    .label("前へ")
                                    .disabled(self.collections_previous.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(cursor) = this.collections_previous.clone() {
                                            this.load_collections(
                                                PagePosition::Before(cursor),
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .child(
                                button(t, "collections-after")
                                    .icon(IconName::ChevronRight)
                                    .label("次へ")
                                    .disabled(self.collections_next.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(cursor) = this.collections_next.clone() {
                                            this.load_collections(
                                                PagePosition::After(cursor),
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            ),
                    );
            }
            Popup::Leave(nav) => {
                let quit = matches!(nav, Navigation::Quit);
                content = content.child(if self.busy {
                    "保存結果を待ってから終了できます。入力画面にも戻れます。"
                } else {
                    "保存するか、変更を破棄するか選んでください。"
                });
                if self.busy {
                    if quit {
                        content = content.child(
                            button(t, "wait-close")
                                .label("保存完了を待って終了")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.quit_after_save = true;
                                    this.popup = None;
                                    cx.notify();
                                })),
                        );
                    }
                } else {
                    let after = nav.clone();
                    content = content
                        .child(
                            button(t, "save-leave")
                                .primary()
                                .label(if quit {
                                    "保存して終了"
                                } else {
                                    "保存して移動"
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.after_save = Some(after.clone());
                                    this.popup = None;
                                    if this.form.is_some() {
                                        this.save_form(window, cx);
                                    } else {
                                        this.add_task(window, cx);
                                    }
                                    if !this.busy {
                                        this.after_save = None;
                                    }
                                })),
                        )
                        .child(
                            button(t, "discard-leave")
                                .label(if quit {
                                    "保存せず終了"
                                } else {
                                    "変更を破棄"
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.discard_form();
                                    this.navigate_now(nav.clone(), window, cx);
                                })),
                        );
                }
            }
            Popup::Delete(target) => {
                content = content
                    .child("削除すると一覧から取り除かれます。タスクの関連作業枠も削除されます。保存後は元に戻せます。")
                    .child(
                        button(t, "confirm-delete")
                            .danger()
                            .icon(IconName::Trash)
                            .label("削除する")
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.popup = None;
                                let origin = this
                                    .form
                                    .as_ref()
                                    .map(|f| WriteOrigin::Form { stamp: f.stamp })
                                    .unwrap_or(WriteOrigin::Direct);
                                this.write(Work::Delete(target.clone()), origin, window, cx);
                            })),
                    );
            }
        }
        content = content.child(
            button(t, "close-popup")
                .label(if matches!(self.popup, Some(Popup::Leave(_))) {
                    "編集を続ける"
                } else {
                    "キャンセル"
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    if matches!(this.popup, Some(Popup::Date)) {
                        this.date_validation = None;
                    }
                    this.dismiss_popup(window, cx);
                })),
        );
        Some(
            div()
                .id("popup-overlay")
                .absolute()
                .inset_0()
                .bg(gpui_kit::rgba(0x00000030))
                .flex()
                .justify_end()
                .items_start()
                .pt(px(64.))
                .pr_4()
                .occlude()
                .child(
                    content
                        .tab_group()
                        .role(Role::Dialog)
                        .aria_label(dialog_title)
                        .focus_trap("desktop-popup", &self.popup_focus),
                )
                .into_any_element(),
        )
    }
}
