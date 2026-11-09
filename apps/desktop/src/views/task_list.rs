use super::*;

impl TasksView {
    pub(super) fn task_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        range
            .map(|index| {
                let task = snapshot.tasks[index].clone();
                let id = task.id;
                let revision = task.revision;
                let completed = task.completed;
                h_flex()
                    .h(px(48.))
                    .px_3()
                    .gap_2()
                    .w_full()
                    .border_b_1()
                    .border_color(self.tokens.border)
                    .bg(if self.selected == Some(id) {
                        self.tokens.selected
                    } else {
                        self.tokens.canvas
                    })
                    .child(
                        Button::new(SharedString::from(format!("complete-{id}")))
                            .ghost()
                            .small()
                            .label(if completed { "済" } else { "○" })
                            .accessibility_label(format!(
                                "{}を{}",
                                task.title,
                                if completed {
                                    "未完了に戻す"
                                } else {
                                    "完了する"
                                }
                            ))
                            .disabled(self.pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch(
                                    Operation::Execute(Command::SetTaskCompleted {
                                        id,
                                        expected_revision: revision,
                                        completed: !completed,
                                    }),
                                    false,
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("select-{id}")))
                            .ghost()
                            .label(task.title)
                            .flex_1()
                            .min_w_0()
                            .justify_start()
                            .disabled(self.pending)
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.select(id, window, cx)),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    }
}
