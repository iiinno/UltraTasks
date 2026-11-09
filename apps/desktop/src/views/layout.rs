use super::*;

impl Render for TasksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let history = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.history)
            .unwrap_or_default();
        let count = self
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.tasks.len());
        let detail = self.render_detail(window, cx);
        let agenda = self.render_agenda(cx);
        v_flex()
            .size_full()
            .bg(self.tokens.canvas)
            .text_color(self.tokens.primary)
            .p_5()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("UltraTasks"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("undo")
                            .label("元に戻す")
                            .disabled(self.pending || !history.can_undo)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dispatch(Operation::Undo, false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("redo")
                            .label("やり直す")
                            .disabled(self.pending || !history.can_redo)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dispatch(Operation::Redo, false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("motion")
                            .ghost()
                            .label(if self.motion {
                                "動き: ON"
                            } else {
                                "動き: OFF"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.motion = !this.motion;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("refresh")
                            .ghost()
                            .label("再読み込み")
                            .disabled(self.pending)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dispatch(Operation::Refresh, false, window, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .gap_5()
                    .child(
                        v_flex()
                            .w(relative(0.42))
                            .min_w(px(300.))
                            .gap_3()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div().flex_1().child(
                                            Input::new(&self.title)
                                                .aria_label("新しいタスク名")
                                                .disabled(self.pending),
                                        ),
                                    )
                                    .child(
                                        Button::new("add")
                                            .primary()
                                            .label("追加")
                                            .disabled(self.pending)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_task(window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div().flex_1().child(
                                            Input::new(&self.search)
                                                .aria_label("タスクを検索")
                                                .disabled(self.pending),
                                        ),
                                    )
                                    .child(
                                        Button::new("search")
                                            .label("検索")
                                            .disabled(self.pending)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.search_term =
                                                    this.search.read(cx).value().to_string();
                                                this.page_offset = 0;
                                                this.dispatch(
                                                    Operation::Refresh,
                                                    false,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            )
                            .child(
                                div().flex_1().min_h_0().child(
                                    uniform_list("task-list", count, cx.processor(Self::task_rows))
                                        .h_full(),
                                ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_sm()
                                            .text_color(self.tokens.secondary)
                                            .child(format!(
                                                "{}件表示 · {}件目から",
                                                count,
                                                self.page_offset + 1
                                            )),
                                    )
                                    .child(
                                        Button::new("previous-page")
                                            .ghost()
                                            .small()
                                            .label("前")
                                            .disabled(self.pending || self.page_offset == 0)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.page_offset =
                                                    this.page_offset.saturating_sub(PAGE_SIZE);
                                                this.dispatch(
                                                    Operation::Refresh,
                                                    false,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("next-page")
                                            .ghost()
                                            .small()
                                            .label("次")
                                            .disabled(self.pending || count < PAGE_SIZE as usize)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.page_offset =
                                                    this.page_offset.saturating_add(PAGE_SIZE);
                                                this.dispatch(
                                                    Operation::Refresh,
                                                    false,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_lg()
                                            .child(format!("{} · {}", self.date, self.timezone)),
                                    )
                                    .child(
                                        Button::new("previous-day")
                                            .ghost()
                                            .label("前日")
                                            .disabled(self.pending)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.change_day(-1, window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("next-day")
                                            .ghost()
                                            .label("翌日")
                                            .disabled(self.pending)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.change_day(1, window, cx)
                                            })),
                                    ),
                            )
                            .child(detail)
                            .child(agenda),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(if self.is_error {
                        self.tokens.error
                    } else {
                        self.tokens.secondary
                    })
                    .child(self.message.clone()),
            )
    }
}
