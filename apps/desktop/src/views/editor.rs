use crate::app::*;
use gpui_kit::{
    assets::IconName,
    component::{
        Icon, Sizable,
        menu::{DropdownMenu, PopupMenuItem},
        radio::{Radio, RadioGroup},
        select::Select,
    },
};

fn field(
    label: &'static str,
    input: &Entity<InputState>,
    disabled: bool,
    invalid: bool,
    t: Tokens,
    cx: &App,
) -> impl IntoElement {
    div()
        .v_flex()
        .gap(px(6.))
        .min_w_0()
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(t.text_secondary)
                .child(label),
        )
        .child(
            input_frame(input.entity_id(), &input.focus_handle(cx), t).child(
                Styled::h(Input::new(input).small(), px(32.))
                    .text_size(px(13.))
                    .aria_label(label)
                    .disabled(disabled)
                    .bg(t.surface)
                    .border_color(if invalid { t.error_text } else { t.border })
                    .focus_bordered(false),
            ),
        )
}
fn validation(form: &Form, area: InputArea, t: Tokens) -> Option<AnyElement> {
    form.validation
        .as_ref()
        .filter(|v| v.area == area)
        .map(|v| {
            super::validation_message(
                match area {
                    InputArea::Title => "title-error",
                    InputArea::Schedule => "schedule-error",
                    InputArea::Form => "form-error",
                },
                v.message.clone(),
                t,
            )
            .into_any_element()
        })
}
impl Desktop {
    pub(crate) fn editor_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(form) = &self.form else {
            return div().into_any_element();
        };
        let t = self.tokens;
        let draft = &form.draft;
        let is_task = matches!(draft.target, Target::Task { .. });
        let is_block = matches!(draft.target, Target::Block { .. });
        let is_event = matches!(draft.target, Target::Event { .. });
        let has_time = is_block || is_event || is_task && draft.due != DueKind::None;
        let title_invalid = form
            .validation
            .as_ref()
            .is_some_and(|v| v.area == InputArea::Title);
        let schedule_invalid = form
            .validation
            .as_ref()
            .is_some_and(|v| v.area == InputArea::Schedule);
        let fields = form.fields.clone();
        let name = draft.target.name();
        let target_id = draft.target.id();
        let width = f32::from(window.viewport_size().width);
        let sidebar_width = if width < 900. { 48. } else { 244. };
        let available_width = width - sidebar_width - 10.;
        let stacked = available_width < 900.;
        let properties_width = (available_width * 0.26087 + 66.087).clamp(280., 400.);
        let column_gap = (available_width * 0.08696 - 55.304).clamp(16., 56.);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&window.text_style().font());
        let content_max_width = text_system.layout_width(font_id, px(t.body_size), '0') * 80.;
        let has_properties = is_task || is_block || is_event;
        let back_label = if self.calendar_view {
            "時間割に戻る"
        } else {
            "タスク一覧に戻る"
        };
        let mut body = div()
            .v_flex()
            .gap(px(24.))
            .min_w_0()
            .mt(px(if available_width <= 768. { 18. } else { 32. }))
            .pb(px(24.));
        let mut properties = div()
            .id("editor-properties")
            .v_flex()
            .gap(px(24.))
            .min_w_0()
            .pt(px(8.))
            .pb(px(24.))
            .when(stacked && has_properties, |d| {
                d.pt(px(24.)).border_t_1().border_color(t.border)
            });
        if let Target::Block { task, .. } = &draft.target {
            let id = task.key.id;
            body = body.child(
                button(t, "edit-related-task")
                    .ghost()
                    .small()
                    .w_full()
                    .h_auto()
                    .min_h(px(32.))
                    .px(px(0.))
                    .py(px(6.))
                    .mb(px(8.))
                    .accessibility_label(format!("関連タスクを編集 {}", draft.title))
                    .tooltip("関連タスクを開く")
                    .child(
                        div()
                            .h_flex()
                            .items_start()
                            .w_full()
                            .min_w_0()
                            .gap(px(8.))
                            .text_left()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(20.))
                                    .line_height(px(26.6))
                                    .font_weight(FontWeight(590.))
                                    .whitespace_normal()
                                    .child(draft.title.clone()),
                            )
                            .child(
                                Icon::new(IconName::ChevronRight)
                                    .with_size(px(16.))
                                    .mt(px(5.))
                                    .text_color(t.text_secondary)
                                    .flex_shrink_0(),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.navigate(Navigation::Task(id), window, cx)
                    })),
            );
        } else {
            body = body.child(
                div()
                    .v_flex()
                    .gap(px(8.))
                    .mb(px(8.))
                    .child(
                        div()
                            .id("editor-title")
                            .track_focus(&form.title_input.focus_handle(cx))
                            .rounded(px(t.control_radius))
                            .border_1()
                            .border_color(if title_invalid {
                                t.error_text
                            } else {
                                transparent_black()
                            })
                            .focus_visible(move |s| {
                                s.border_color(if title_invalid {
                                    t.error_text
                                } else {
                                    t.focus_ring
                                })
                            })
                            .child(
                                Textarea::new(&form.title_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .px(px(0.))
                                    .py(px(6.))
                                    .text_size(px(20.))
                                    .line_height(px(26.6))
                                    .font_weight(FontWeight(590.))
                                    .text_color(t.text_primary)
                                    .bg(t.canvas)
                                    .h(px(80.))
                                    .aria_label("タイトル")
                                    .on_paste({
                                        let weak = cx.weak_entity();
                                        let serial = form.stamp.serial;
                                        move |_, window, _| {
                                            let weak = weak.clone();
                                            window.on_next_frame(move |_, cx| {
                                                let _ = weak.update(cx, |this, _| {
                                                    if let Some(form) = &mut this.form
                                                        && form.stamp.serial == serial
                                                    {
                                                        form.submit_ready = true;
                                                    }
                                                });
                                            });
                                            false
                                        }
                                    })
                                    .disabled(self.busy),
                            ),
                    )
                    .children(validation(form, InputArea::Title, t)),
            );
        }
        if is_task {
            properties = properties
                .child(
                    div()
                        .id("task-state-field")
                        .role(Role::Group)
                        .aria_label("状態")
                        .v_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(t.text_secondary)
                                .child("状態"),
                        )
                        .child(
                            RadioGroup::horizontal("task-state")
                                .children(["未完了", "完了"].map(|label| {
                                    Radio::new(label).label(label).small().text_size(px(13.))
                                }))
                                .selected_index(Some(usize::from(draft.status == TaskStatus::Done)))
                                .disabled(self.busy)
                                .on_change(cx.listener(|this, index, _, cx| {
                                    if let Some(form) = &mut this.form {
                                        form.draft.status = if *index == 0 {
                                            TaskStatus::Open
                                        } else {
                                            TaskStatus::Done
                                        };
                                        form.changed();
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .id("due-kind-field")
                        .role(Role::Group)
                        .aria_label("期限")
                        .v_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(t.text_secondary)
                                .child("期限"),
                        )
                        .child(
                            RadioGroup::horizontal("due-kind")
                                .children(["なし", "日付", "日時"].map(|label| {
                                    Radio::new(label).label(label).small().text_size(px(13.))
                                }))
                                .selected_index(Some(match draft.due {
                                    DueKind::None => 0,
                                    DueKind::Date => 1,
                                    DueKind::At => 2,
                                }))
                                .disabled(self.busy)
                                .on_change(cx.listener(|this, index, _, cx| {
                                    if let Some(form) = &mut this.form {
                                        form.draft.due = match index {
                                            0 => DueKind::None,
                                            1 => DueKind::Date,
                                            _ => DueKind::At,
                                        };
                                        form.changed();
                                    }
                                    cx.notify();
                                })),
                        ),
                );
        }
        if is_event {
            properties = properties.child(
                div()
                    .id("event-kind-field")
                    .role(Role::Group)
                    .aria_label("時間")
                    .v_flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(t.text_secondary)
                            .child("時間"),
                    )
                    .child(
                        RadioGroup::horizontal("event-kind")
                            .children(["時刻付き", "終日"].map(|label| {
                                Radio::new(label).label(label).small().text_size(px(13.))
                            }))
                            .selected_index(Some(usize::from(draft.all_day)))
                            .disabled(self.busy)
                            .on_change(cx.listener(|this, index, _, cx| {
                                if let Some(form) = &mut this.form {
                                    form.draft.all_day = *index == 1;
                                    form.changed();
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        if has_time {
            let with_clock = !draft.all_day && (!is_task || draft.due == DueKind::At);
            let mut schedule = div().v_flex().gap(px(12.)).child(
                div()
                    .h_flex()
                    .items_end()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(field(
                        if is_task { "期限日" } else { "開始日" },
                        &fields[0],
                        self.busy,
                        schedule_invalid,
                        t,
                        cx,
                    )))
                    .when(with_clock, |d| {
                        d.child(div().w(px(112.)).flex_shrink_0().child(field(
                            if is_task {
                                "期限時刻"
                            } else {
                                "開始時刻"
                            },
                            &fields[1],
                            self.busy,
                            schedule_invalid,
                            t,
                            cx,
                        )))
                    }),
            );
            if !is_task {
                schedule = schedule.child(
                    div()
                        .h_flex()
                        .items_end()
                        .gap(px(8.))
                        .child(div().flex_1().min_w_0().child(field(
                            if draft.all_day {
                                "最終日（この日まで）"
                            } else {
                                "終了日"
                            },
                            &fields[2],
                            self.busy,
                            schedule_invalid,
                            t,
                            cx,
                        )))
                        .when(with_clock, |d| {
                            d.child(div().w(px(112.)).flex_shrink_0().child(field(
                                "終了時刻",
                                &fields[3],
                                self.busy,
                                schedule_invalid,
                                t,
                                cx,
                            )))
                        }),
                );
            }
            schedule = schedule.children(validation(form, InputArea::Schedule, t));
            if with_clock {
                schedule = schedule.child(
                    div()
                        .v_flex()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(t.text_secondary)
                        .child("YYYY-MM-DD · 9:00 / 9時")
                        .child(self.options.zone.as_str().to_owned()),
                );
                for end in [false, true] {
                    if end && is_task {
                        continue;
                    }
                    let index = if end { 2 } else { 0 };
                    if let Some((earlier, later)) = crate::presentation::editor::overlap_options(
                        &fields[index].read(cx).value(),
                        &fields[index + 1].read(cx).value(),
                        &self.options.zone,
                    ) {
                        let selected = if end {
                            draft.end_overlap
                        } else {
                            draft.start_overlap
                        };
                        schedule = schedule.child(
                            div()
                                .v_flex()
                                .gap(px(8.))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .child(if end {
                                    "終了時刻が二度あります"
                                } else {
                                    "開始・期限時刻が二度あります"
                                })
                                .child(
                                    RadioGroup::vertical(if end {
                                        "end-overlap"
                                    } else {
                                        "start-overlap"
                                    })
                                    .children(
                                        [earlier, later].map(|label| {
                                            Radio::from(label).small().text_size(px(13.))
                                        }),
                                    )
                                    .selected_index(match selected {
                                        OverlapChoice::Reject => None,
                                        OverlapChoice::Earlier => Some(0),
                                        OverlapChoice::Later => Some(1),
                                    })
                                    .disabled(self.busy)
                                    .on_change(cx.listener(move |this, index, _, cx| {
                                        if let Some(form) = &mut this.form {
                                            let choice = if *index == 0 {
                                                OverlapChoice::Earlier
                                            } else {
                                                OverlapChoice::Later
                                            };
                                            if end {
                                                form.draft.end_overlap = choice;
                                            } else {
                                                form.draft.start_overlap = choice;
                                            }
                                            form.changed();
                                        }
                                        cx.notify();
                                    })),
                                ),
                        );
                    }
                }
            }
            properties = properties.child(schedule);
        }
        if is_task || is_block || is_event {
            let label = if is_task {
                "リスト"
            } else {
                "カレンダー"
            };
            properties = properties.child(
                div()
                    .v_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(t.text_secondary)
                            .child(label),
                    )
                    .child(
                        Select::new(&form.collection)
                            .small()
                            .h(px(32.))
                            .text_size(px(13.))
                            .accessibility_label(label)
                            .placeholder("所属先を選ぶ")
                            .disabled(self.busy)
                            .w_full(),
                    ),
            );
        }
        if let Target::Task {
            id,
            blocks,
            expected,
        } = &draft.target
        {
            let mut work = div()
                .v_flex()
                .gap(px(6.))
                .pt(px(16.))
                .border_t_1()
                .border_color(t.border)
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child("作業時間"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .child(format!("{}件", blocks.len())),
                        ),
                );
            if blocks.is_empty() {
                work = work.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(t.text_secondary)
                        .child("作業時間は未設定です"),
                );
            }
            for (index, block) in blocks.iter().enumerate() {
                let id = block.key.id;
                let label = time::span_label(&block.data.span, &self.options.zone);
                work = work.child(
                    button(t, ("related-block", index))
                        .ghost()
                        .small()
                        .w_full()
                        .h_auto()
                        .min_h(px(32.))
                        .px(px(8.))
                        .py(px(6.))
                        .accessibility_label(format!("作業時間を編集 {label}"))
                        .tooltip(label.clone())
                        .child(
                            div()
                                .h_flex()
                                .w_full()
                                .min_w_0()
                                .gap(px(8.))
                                .child(Icon::new(IconName::Clock).with_size(px(14.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_left()
                                        .whitespace_normal()
                                        .child(label),
                                )
                                .child(
                                    Icon::new(IconName::ChevronRight)
                                        .with_size(px(14.))
                                        .text_color(t.text_secondary),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.navigate(
                                Navigation::Entity(core::EntityId::TimeBlock(id)),
                                window,
                                cx,
                            )
                        })),
                );
            }
            let id = *id;
            let exists = expected.is_some();
            work = work.child(
                div().h_flex().child(
                    button(t, "add-block")
                        .ghost()
                        .small()
                        .compact()
                        .h(px(28.))
                        .icon(IconName::Plus)
                        .label("作業時間を追加")
                        .disabled(self.busy || !exists)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.form.as_ref().is_some_and(|f| f.dirty) {
                                this.fail(
                                    "作業時間を追加する前にタスクの変更を保存してください".into(),
                                    cx,
                                );
                                return;
                            }
                            let Some(form) = &this.form else {
                                return;
                            };
                            let Target::Task {
                                expected: Some(key),
                                ..
                            } = form.draft.target
                            else {
                                return;
                            };
                            let Some(info) = &this.info else {
                                return;
                            };
                            match (TimeBlockId::new(), Title::new(&form.draft.title)) {
                                (Ok(block_id), Ok(title)) => {
                                    let task = TaskSummary {
                                        key: Expected {
                                            id,
                                            revision: key.revision,
                                        },
                                        title: title.clone(),
                                        status: form.draft.status,
                                    };
                                    let mut draft = Draft::blank(
                                        Target::Block {
                                            id: block_id,
                                            expected: None,
                                            task,
                                        },
                                        this.options.day,
                                        info,
                                    );
                                    draft.title = title.as_str().into();
                                    this.set_form(draft, window, cx);
                                }
                                (Err(error), _) | (_, Err(error)) => {
                                    this.fail(user_error(&error), cx)
                                }
                            }
                        })),
                ),
            );
            body = body.child(work);
        }
        properties = properties.children(validation(form, InputArea::Form, t));
        if let Some(conflict) = &form.conflict {
            properties = properties.child(
                div()
                    .v_flex()
                    .gap(px(8.))
                    .p(px(12.))
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .bg(t.warning_background)
                    .text_color(t.warning_text)
                    .child("別の更新があるため保存していません")
                    .child(conflict.clone())
                    .child("下書きは入力欄に残っています")
                    .child(
                        button(t, "copy-draft")
                            .ghost()
                            .small()
                            .compact()
                            .h(px(28.))
                            .icon(IconName::Copy)
                            .label("下書きをコピー")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(form) = &this.form {
                                    let text = std::iter::once(
                                        form.title_input.read(cx).value().to_string(),
                                    )
                                    .chain(
                                        form.fields.iter().map(|f| f.read(cx).value().to_string()),
                                    )
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                            })),
                    )
                    .child(
                        button(t, "reload-entity")
                            .ghost()
                            .small()
                            .compact()
                            .h(px(28.))
                            .icon(IconName::RotateCcw)
                            .label("下書きを破棄して最新を開く")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.discard_form();
                                this.navigate_now(Navigation::Entity(target_id), window, cx);
                            })),
                    ),
            );
        }
        let content = if stacked || !has_properties {
            div()
                .v_flex()
                .w_full()
                .max_w(content_max_width)
                .mx_auto()
                .px(px(16.))
                .pt(px(21.5))
                .child(body.w_full())
                .child(properties.w_full())
        } else {
            div()
                .h_flex()
                .items_start()
                .justify_center()
                .w_full()
                .px(px(column_gap))
                .pr(px(column_gap + 14.))
                .pt(px(21.5))
                .gap(px(column_gap))
                .child(body.flex_1().max_w(content_max_width))
                .child(properties.w(px(properties_width)).flex_shrink_0())
        };
        let delete = draft.target.delete();
        let weak = cx.weak_entity();
        let actions = button(t, "editor-actions")
            .ghost()
            .small()
            .size(px(28.))
            .icon(IconName::Ellipsis)
            .accessibility_label("編集の操作")
            .tooltip("編集の操作")
            .disabled(self.busy)
            .map(|trigger| super::menu_trigger("editor-menu-focus", trigger, self.busy, window, cx))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let cancel_view = weak.clone();
                let menu = menu.item(PopupMenuItem::new("取消").icon(IconName::X).on_click(
                    move |_, window, cx| {
                        let _ = cancel_view.update(cx, |this, cx| {
                            if this.busy {
                                return;
                            }
                            let navigation = if this.calendar_view {
                                Navigation::Calendar
                            } else {
                                Navigation::Tasks
                            };
                            this.navigate(navigation, window, cx);
                        });
                    },
                ));
                if let Some(target) = delete.clone() {
                    let delete_view = weak.clone();
                    menu.item(PopupMenuItem::separator()).item(
                        PopupMenuItem::new("削除").icon(IconName::Trash).on_click(
                            move |_, window, cx| {
                                let _ = delete_view.update(cx, |this, cx| {
                                    if !this.busy {
                                        this.open_popup(Popup::Delete(target.clone()), window, cx);
                                    }
                                });
                            },
                        ),
                    )
                } else {
                    menu
                }
            });
        let panel = div()
            .v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(t.canvas)
            .child(
                div()
                    .h_flex()
                    .h(px(t.header_height))
                    .flex_shrink_0()
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        button(t, "back-tasks")
                            .ghost()
                            .small()
                            .size(px(28.))
                            .icon(IconName::ArrowLeft)
                            .accessibility_label(back_label)
                            .tooltip(back_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let navigation = if this.calendar_view {
                                    Navigation::Calendar
                                } else {
                                    Navigation::Tasks
                                };
                                this.navigate(navigation, window, cx)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .when(self.busy || form.dirty, |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .child(if self.busy { "保存中" } else { "未保存" }),
                        )
                    })
                    .child(
                        button(t, "save-editor")
                            .primary()
                            .small()
                            .h(px(28.))
                            .px(px(12.))
                            .label("保存")
                            .tooltip("保存 · Ctrl+Enter")
                            .disabled(self.busy || self.recovery || form.conflict.is_some())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_form(window, cx)),
                            ),
                    )
                    .child(actions),
            )
            .child(
                div()
                    .id("editor-body")
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            );
        if self.motion.fade_ms > 0 {
            panel
                .with_animation(
                    ("editor-fade", self.form_serial),
                    Animation::new(std::time::Duration::from_millis(self.motion.fade_ms as u64)),
                    |panel, delta| panel.opacity(delta),
                )
                .into_any_element()
        } else {
            panel.into_any_element()
        }
    }
}
