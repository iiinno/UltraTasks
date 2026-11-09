use super::*;

impl TasksView {
    pub(super) fn render_agenda(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut rows = Vec::new();
        if let Some(snapshot) = &self.snapshot {
            for item in &snapshot.blocks {
                let block = item.block.clone();
                let remove = block.clone();
                let local_start = block.start.with_timezone(&self.timezone);
                let local_end = block.end.with_timezone(&self.timezone);
                let label = format!(
                    "{} – {}",
                    local_start.format("%H:%M"),
                    local_end.format("%m/%d %H:%M")
                );
                let panel = h_flex()
                    .relative()
                    .p_3()
                    .gap_3()
                    .bg(self.tokens.surface)
                    .rounded_md()
                    .child(
                        v_flex().flex_1().gap_1().child(item.title.clone()).child(
                            div()
                                .text_sm()
                                .text_color(self.tokens.secondary)
                                .child(label),
                        ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("move-{}", block.id)))
                            .ghost()
                            .small()
                            .label("+30分")
                            .disabled(self.pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.move_block(block.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-{}", remove.id)))
                            .ghost()
                            .small()
                            .label("解除")
                            .disabled(self.pending)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch(
                                    Operation::Execute(Command::DeleteTimeBlock {
                                        id: remove.id,
                                        expected_revision: remove.revision,
                                    }),
                                    false,
                                    window,
                                    cx,
                                )
                            })),
                    );
                rows.push(if self.motion {
                    panel
                        .with_animation(
                            ElementId::NamedInteger(
                                format!("block-motion-{}", item.block.id).into(),
                                item.block.revision,
                            ),
                            Animation::new(self.tokens.move_block),
                            |panel, progress| panel.top(px(-12. * (1. - progress))),
                        )
                        .into_any_element()
                } else {
                    panel.into_any_element()
                });
            }
            for event in &snapshot.events {
                let schedule = match &event.schedule {
                    EventSchedule::AllDay { start, end } => {
                        format!("終日 {} – {}（終了日を含まない）", start, end)
                    }
                    EventSchedule::Timed { start, end, .. } => format!(
                        "{} – {}",
                        start.with_timezone(&self.timezone).format("%H:%M"),
                        end.with_timezone(&self.timezone).format("%m/%d %H:%M")
                    ),
                };
                rows.push(
                    v_flex()
                        .p_3()
                        .gap_1()
                        .bg(self.tokens.surface)
                        .rounded_md()
                        .child(event.title.clone())
                        .child(
                            div()
                                .text_sm()
                                .text_color(self.tokens.secondary)
                                .child(schedule),
                        )
                        .into_any_element(),
                );
            }
            if rows.is_empty() {
                rows.push(
                    div()
                        .p_4()
                        .text_color(self.tokens.secondary)
                        .child("この日の予定はありません")
                        .into_any_element(),
                );
            }
            if snapshot.events.len() == PAGE_SIZE as usize
                || snapshot.blocks.len() == PAGE_SIZE as usize
            {
                rows.push(
                    div()
                        .text_color(self.tokens.error)
                        .child("表示上限に達しました。この仮画面は各種1000件まで表示します")
                        .into_any_element(),
                );
            }
        }
        v_flex()
            .id("day-agenda")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_2()
            .children(rows)
            .into_any_element()
    }
}
