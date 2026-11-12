use crate::app::*;
use gpui_kit::{
    assets::IconName,
    component::{Icon, Sizable, tooltip::Tooltip},
};

impl Desktop {
    pub(crate) fn agenda_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = self.tokens;
        let day = match time::calendar(self.options.day, &self.options.zone) {
            Ok(day) => day,
            Err(e) => return div().p_4().child(user_error(&e)).into_any_element(),
        };
        let (base, end) = day.instant_bounds();
        let day_minutes = (end.unix_micros() - base.unix_micros()) as f32 / time::MINUTE as f32;
        let top = (-f32::from(self.agenda_scroll.offset().y)).max(0.);
        let viewport = f32::from(window.viewport_size().height);
        let width = f32::from(self.agenda_scroll.bounds().size.width).max(300.);
        let content_width = (width - 64.).max(180.);
        let start_hour = ((top - 60.).max(0.) / 60.).floor() as i64;
        let end_hour = ((top + viewport + 60.) / 60.).ceil() as i64;
        let mut grid = div()
            .relative()
            .w_full()
            .h(px(day_minutes.max(1.)))
            .min_h(px(1.));
        for hour in start_hour..=end_hour {
            let minute = hour * 60;
            if minute as f32 > day_minutes {
                continue;
            }
            if let Ok(at) =
                core::Instant::from_unix_micros(base.unix_micros() + minute * time::MINUTE)
            {
                let label = if (day_minutes - 1440.).abs() > 0.1 {
                    time::instant_label(at, &self.options.zone)
                        .split_once(' ')
                        .map(|(_, tail)| tail.to_string())
                        .unwrap_or_default()
                } else {
                    time::clock_label(at, &self.options.zone)
                };
                grid = grid.child(
                    div()
                        .absolute()
                        .top(px(minute as f32))
                        .left_0()
                        .right_0()
                        .h(px(1.))
                        .child(
                            div()
                                .absolute()
                                .top(px(if minute == 0 { 0. } else { -8. }))
                                .line_height(px(16.))
                                .w(px(56.))
                                .when((day_minutes - 1440.).abs() <= 0.1, |d| {
                                    d.w(px(48.)).text_right()
                                })
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .child(label),
                        )
                        .child(div().absolute().left(px(56.)).right_0().h(px(1.)).bg(
                            if t.canvas == t.surface_hover {
                                t.border
                            } else {
                                t.surface_hover
                            },
                        )),
                );
            }
        }
        for card in time::cards(&self.agenda, &day, content_width) {
            let placement = card.placement;
            if placement.end < top - 60. || placement.start > top + viewport + 60. {
                continue;
            }
            let item = self.agenda[placement.index].clone();
            let kind = match &item {
                AgendaItem::Event(_) => "予定",
                AgendaItem::TimeBlock { task, .. } if task.status == TaskStatus::Done => {
                    "作業・完了"
                }
                _ => "作業",
            };
            let title = time::title(&item).to_owned();
            let Some(span) = time::span(&item) else {
                continue;
            };
            let span = span.clone();
            let (a, b) = span.bounds();
            let continuation = format!(
                "{}{}",
                if a < base { "前日から · " } else { "" },
                if b > end { " · 翌日へ" } else { "" }
            );
            let label = format!(
                "{}–{}{}",
                time::clock_label(a, &self.options.zone),
                time::clock_label(b, &self.options.zone),
                continuation
            );
            let lane_width = content_width / placement.lanes.max(1) as f32;
            if let Some(count) = card.candidates {
                let start = core::Instant::from_unix_micros(
                    base.unix_micros() + (placement.start as i64) * time::MINUTE,
                )
                .unwrap_or(base);
                let stop = core::Instant::from_unix_micros(
                    base.unix_micros() + (placement.end as i64) * time::MINUTE,
                )
                .unwrap_or(end);
                let label = format!(
                    "{}–{} · 重なる予定・作業枠 {}件",
                    time::clock_label(start, &self.options.zone),
                    time::clock_label(stop, &self.options.zone),
                    count
                );
                grid = grid.child(
                    button(t, ("dense", placement.index))
                        .ghost()
                        .small()
                        .compact()
                        .absolute()
                        .top(px(placement.start))
                        .left(px(56.))
                        .right(px(8.))
                        .h(px(placement.end - placement.start))
                        .bg(t.surface)
                        .border_l_1()
                        .border_color(t.text_secondary)
                        .rounded(px(3.))
                        .text_size(px(12.))
                        .accessibility_label(format!("{label} 候補一覧を開く"))
                        .tooltip("表示中の予定・作業枠を候補一覧で選ぶ")
                        .label(label)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.agenda_candidates = true;
                            cx.notify();
                        })),
                );
                continue;
            }
            let related = match &item {
                AgendaItem::TimeBlock { task, .. } => self.selected == Some(task.key.id),
                _ => false,
            };
            let block = match &item {
                AgendaItem::TimeBlock { block, task } => Some((block.clone(), task.clone())),
                _ => None,
            };
            let key = SharedString::from(format!("agenda-{:?}", time::id(&item)));
            let actual_height = placement.end - placement.start;
            let accessible_label = format!(
                "{kind} {title} {} {}",
                self.options.day.iso(),
                time::span_label(&span, &self.options.zone)
            );
            let item_click = item.clone();
            let card = div()
                .id(key)
                .absolute()
                .top(px(placement.start))
                .left(px(56. + placement.lane as f32 * lane_width))
                .w(px(lane_width - 6.))
                .h(px(actual_height))
                .bg(if related {
                    t.selection_background
                } else {
                    t.surface
                })
                .border_l_1()
                .border_color(if block.is_some() {
                    t.focus_ring
                } else {
                    t.text_secondary
                })
                .rounded(px(3.))
                .text_color(if related {
                    t.selection_text
                } else {
                    t.text_primary
                })
                .hover(move |s| {
                    s.border_color(t.focus_ring).bg(if related {
                        t.selection_background
                    } else {
                        t.surface_hover
                    })
                })
                .focusable()
                .focus(move |s| s.shadow(focus_outline(t)))
                .when(actual_height >= 18., |d| d.overflow_hidden())
                .px(px(8.))
                .cursor_pointer()
                .role(Role::Button)
                .aria_label(accessible_label.clone())
                .tooltip(move |window, cx| {
                    Tooltip::new(accessible_label.clone())
                        .max_w(px(420.))
                        .build(window, cx)
                })
                .map(|d| {
                    if time::compact_card(actual_height) {
                        d.child(
                            div()
                                .h_flex()
                                .min_w_0()
                                .gap(px(8.))
                                .h(px(actual_height))
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .child(
                                    Icon::new(if block.is_some() {
                                        IconName::Clock
                                    } else {
                                        IconName::Calendar
                                    })
                                    .with_size(px(12.))
                                    .flex_shrink_0(),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_weight(FontWeight::MEDIUM)
                                        .truncate()
                                        .child(title.clone()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .max_w(relative(0.5))
                                        .text_color(if related {
                                            t.selection_text
                                        } else {
                                            t.text_secondary
                                        })
                                        .truncate()
                                        .child(label.clone()),
                                ),
                        )
                    } else {
                        d.child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(20.))
                                .font_weight(FontWeight::MEDIUM)
                                .line_clamp(if actual_height >= 60. { 2 } else { 1 })
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .h_flex()
                                .min_w_0()
                                .gap(px(4.))
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(if related {
                                    t.selection_text
                                } else {
                                    t.text_secondary
                                })
                                .child(
                                    Icon::new(if block.is_some() {
                                        IconName::Clock
                                    } else {
                                        IconName::Calendar
                                    })
                                    .with_size(px(12.))
                                    .flex_shrink_0(),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .child(format!("{kind} · {label}")),
                                ),
                        )
                    }
                })
                .on_key_down({
                    let id = time::id(&item);
                    cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                            cx.stop_propagation();
                            this.navigate(Navigation::Entity(id), window, cx);
                        }
                    })
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if let Some((block, task)) = &block {
                            this.start_drag(
                                DragSource::Block(block.clone(), task.clone()),
                                event.position,
                                window,
                                cx,
                            );
                        }
                    }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !this.suppress_click && !this.drag.as_ref().is_some_and(|d| d.active) {
                        this.navigate(Navigation::Entity(time::id(&item_click)), window, cx);
                    }
                }));
            let card = if let Some(before) = self
                .moved_from
                .get(&time::id(&item))
                .filter(|_| self.motion.move_ms > 0)
            {
                let from = (before.unix_micros() - base.unix_micros()) as f32 / time::MINUTE as f32;
                let to = placement.start;
                card.with_animation(
                    SharedString::from(format!(
                        "move-{:?}-{}",
                        time::id(&item),
                        self.commit_generation
                    )),
                    Animation::new(std::time::Duration::from_millis(self.motion.move_ms as u64)),
                    move |card, progress| card.top(px(from + (to - from) * progress)),
                )
                .into_any_element()
            } else {
                card.into_any_element()
            };
            grid = grid.child(card);
        }
        if let Some(drag) = &self.drag
            && let Some(minute) = drag.minute
        {
            grid = grid.child(
                div()
                    .absolute()
                    .top(px(minute as f32))
                    .left(px(56.))
                    .right(px(8.))
                    .h(px(30.))
                    .px(px(8.))
                    .text_size(px(12.))
                    .line_height(px(28.))
                    .bg(t.selection_background)
                    .text_color(t.selection_text)
                    .border_1()
                    .border_color(t.focus_ring)
                    .child("仮配置 · 解放して時刻を確認"),
            );
        }
        if let Some(now) = self.now
            && now >= base
            && now < end
        {
            let minute = (now.unix_micros() - base.unix_micros()) as f32 / time::MINUTE as f32;
            grid = grid.child(
                div()
                    .absolute()
                    .top(px(minute))
                    .left(px(52.))
                    .right_0()
                    .h(px(2.))
                    .bg(t.focus_ring),
            );
        }
        let mut all_day = div()
            .h_flex()
            .gap(px(8.))
            .min_h(px(36.))
            .flex_shrink_0()
            .py(px(4.))
            .pr(px(8.))
            .border_b_1()
            .border_color(t.border)
            .overflow_hidden()
            .text_size(px(12.))
            .text_color(t.text_secondary)
            .child(div().w(px(48.)).flex_shrink_0().text_right().child("終日"));
        if self.all_day.is_empty() {
            all_day = all_day.child(if self.all_day_loading {
                "読み込み中…"
            } else if !self.all_day_loaded {
                "未取得・更新を確認してください"
            } else {
                "終日の予定なし"
            });
        }
        for (index, item) in self.all_day.iter().take(3).enumerate() {
            let item = item.clone();
            all_day = all_day.child(
                button(t, ("all-day", index))
                    .ghost()
                    .small()
                    .compact()
                    .h(px(28.))
                    .icon(IconName::Calendar)
                    .flex_1()
                    .min_w_0()
                    .tooltip(time::title(&item).to_owned())
                    .accessibility_label(format!("終日 {}", time::title(&item)))
                    .label(time::title(&item).to_owned())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.navigate(Navigation::Entity(time::id(&item)), window, cx)
                    })),
            );
        }
        if self.all_day_total > 3 {
            all_day = all_day.child(
                button(t, "more-all-day")
                    .ghost()
                    .small()
                    .compact()
                    .h(px(28.))
                    .label(format!("全{}件", self.all_day_total))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.agenda_candidates = true;
                        cx.notify();
                    })),
            );
        }
        let body = if self.agenda_candidates {
            let mut items = div()
                .id("agenda-candidates")
                .v_flex()
                .flex_1()
                .min_h_0()
                .px(px(12.))
                .py(px(4.))
                .overflow_y_scroll();
            for (index, item) in self.all_day.iter().chain(self.agenda.iter()).enumerate() {
                let item = item.clone();
                let time_label = time::span(&item)
                    .map(|s| time::span_label(s, &self.options.zone))
                    .unwrap_or_else(|| "終日".into());
                let kind = match &item {
                    AgendaItem::Event(_) => "予定",
                    AgendaItem::TimeBlock { task, .. } if task.status == TaskStatus::Done => {
                        "作業・完了"
                    }
                    _ => "作業",
                };
                let label = format!("{kind} {time_label} · {}", time::title(&item));
                items = items.child(
                    button(t, ("candidate", index))
                        .ghost()
                        .small()
                        .w_full()
                        .h_auto()
                        .min_h(px(56.))
                        .px(px(8.))
                        .py(px(8.))
                        .border_b_1()
                        .border_color(t.border)
                        .accessibility_label(label.clone())
                        .tooltip(label)
                        .child(
                            div()
                                .v_flex()
                                .w_full()
                                .min_w_0()
                                .gap(px(2.))
                                .text_left()
                                .whitespace_normal()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .line_clamp(2)
                                        .child(time::title(&item).to_owned()),
                                )
                                .child(
                                    div()
                                        .h_flex()
                                        .gap(px(4.))
                                        .text_size(px(12.))
                                        .line_height(px(18.))
                                        .text_color(t.text_secondary)
                                        .child(
                                            Icon::new(if matches!(&item, AgendaItem::Event(_)) {
                                                IconName::Calendar
                                            } else {
                                                IconName::Clock
                                            })
                                            .with_size(px(12.))
                                            .flex_shrink_0(),
                                        )
                                        .child(format!("{kind} · {time_label}")),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.navigate(Navigation::Entity(time::id(&item)), window, cx)
                        })),
                );
            }
            items
                .child(
                    div()
                        .h_flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .py(px(8.))
                        .child(
                            button(t, "next-agenda")
                                .ghost()
                                .small()
                                .compact()
                                .h(px(28.))
                                .icon(IconName::Clock)
                                .label("時刻付きの続きを表示")
                                .disabled(self.agenda_next.is_none() || self.agenda_loading)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.query_agenda(false, true, window, cx)
                                })),
                        )
                        .child(
                            button(t, "next-all-day")
                                .ghost()
                                .small()
                                .compact()
                                .h(px(28.))
                                .icon(IconName::Calendar)
                                .label("終日の続きを表示")
                                .disabled(self.all_day_next.is_none() || self.all_day_loading)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.query_agenda(true, true, window, cx)
                                })),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .id("agenda-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.agenda_scroll)
                .on_scroll(cx.listener(|this, _, window, cx| {
                    let weak = cx.weak_entity();
                    window.on_next_frame(move |window, cx| {
                        let _ = weak.update(cx, |this, cx| this.agenda_scrolled(window, cx));
                    });
                    let _ = this;
                }))
                .child(grid)
                .into_any_element()
        };
        div()
            .v_flex()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .h_flex()
                    .h(px(40.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("時間割"),
                    )
                    .when(self.agenda_loaded, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.))
                                .text_color(t.text_secondary)
                                .child(format!("{}件", self.agenda_total)),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(t.text_secondary)
                            .truncate()
                            .child(self.options.zone.as_str().to_owned()),
                    )
                    .child(
                        button(t, "agenda-view")
                            .ghost()
                            .small()
                            .compact()
                            .h(px(28.))
                            .icon(if self.agenda_candidates {
                                IconName::Clock
                            } else {
                                IconName::List
                            })
                            .label(if self.agenda_candidates {
                                "時間軸"
                            } else {
                                "候補一覧"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.agenda_candidates = !this.agenda_candidates;
                                cx.notify();
                            })),
                    ),
            )
            .child(all_day)
            .when(
                self.agenda_loaded && self.agenda_total == 0 && !self.agenda_loading,
                |d| {
                    d.child(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .text_size(px(12.))
                            .text_color(t.text_secondary)
                            .child("この時間帯に予定はありません"),
                    )
                },
            )
            .when(self.agenda_loading, |d| {
                d.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(px(12.))
                        .text_color(t.text_secondary)
                        .child("表示範囲を読み込み中…"),
                )
            })
            .when(self.agenda_next.is_some(), |d| {
                d.child(
                    button(t, "dense-more")
                        .ghost()
                        .small()
                        .compact()
                        .h(px(28.))
                        .mx(px(12.))
                        .my(px(4.))
                        .icon(IconName::List)
                        .label(format!(
                            "この範囲は{}件 · 未取得の候補あり",
                            self.agenda_total
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.agenda_candidates = true;
                            cx.notify();
                        })),
                )
            })
            .child(body)
            .into_any_element()
    }
}
