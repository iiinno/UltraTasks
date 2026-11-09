use super::*;

impl TasksView {
    pub(super) fn render_detail(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(task) = self.selected_task() else {
            return div()
                .p_4()
                .text_color(self.tokens.secondary)
                .child("タスクを選ぶと、編集と時間の割り当てができます")
                .into_any_element();
        };
        let rename = task.clone();
        let delete = task;
        let panel = v_flex()
            .p_4()
            .gap_3()
            .bg(self.tokens.surface)
            .rounded_lg()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().flex_1().child(
                            Input::new(&self.edit_title)
                                .aria_label("選択したタスクの名前")
                                .disabled(self.pending),
                        ),
                    )
                    .child(
                        Button::new("rename")
                            .label("保存")
                            .disabled(self.pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch(
                                    Operation::Execute(Command::UpdateTask {
                                        id: rename.id,
                                        expected_revision: rename.revision,
                                        title: this.edit_title.read(cx).value().to_string(),
                                        completed: rename.completed,
                                        due: rename.due.clone(),
                                        list_id: rename.list_id,
                                    }),
                                    false,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("delete")
                            .ghost()
                            .label("削除")
                            .disabled(self.pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch(
                                    Operation::Execute(Command::DeleteTask {
                                        id: delete.id,
                                        expected_revision: delete.revision,
                                    }),
                                    false,
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child("開始"))
                    .child(
                        div().w(px(94.)).child(
                            Input::new(&self.start_time)
                                .aria_label("作業枠の開始時刻 HH:MM")
                                .disabled(self.pending),
                        ),
                    )
                    .child(div().text_sm().child("分"))
                    .child(
                        div().w(px(72.)).child(
                            Input::new(&self.minutes)
                                .aria_label("作業枠の所要時間 分")
                                .disabled(self.pending),
                        ),
                    )
                    .child(
                        Button::new("schedule")
                            .primary()
                            .label("時間を確保")
                            .disabled(self.pending)
                            .on_click(cx.listener(|this, _, window, cx| this.schedule(window, cx))),
                    ),
            );
        if self.motion {
            panel
                .with_animation(
                    ElementId::NamedInteger("detail-fade".into(), self.detail_epoch),
                    Animation::new(self.tokens.fade),
                    |panel, progress| panel.opacity(progress),
                )
                .into_any_element()
        } else {
            panel.into_any_element()
        }
    }
}
