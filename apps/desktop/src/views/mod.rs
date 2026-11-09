mod agenda;
mod detail;
mod layout;
mod task_list;

use std::ops::Range;

use chrono::{Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use gpui_kit::{
    component::{
        button::*,
        input::{Input, InputEvent, InputState},
        *,
    },
    *,
};
use ultratasks_core::{Command, EventSchedule, Task as CoreTask, TaskId, TimeBlock};

use crate::{
    backend::{Backend, Operation, PAGE_SIZE, Snapshot, ViewQuery},
    presentation::schedule_range,
    theme::ThemeTokens,
};

pub struct TasksView {
    backend: Backend,
    snapshot: Option<Snapshot>,
    title: Entity<InputState>,
    search: Entity<InputState>,
    edit_title: Entity<InputState>,
    start_time: Entity<InputState>,
    minutes: Entity<InputState>,
    selected: Option<TaskId>,
    date: NaiveDate,
    timezone: Tz,
    search_term: String,
    page_offset: u32,
    pending: bool,
    message: SharedString,
    is_error: bool,
    tokens: ThemeTokens,
    motion: bool,
    detail_epoch: u64,
    _subscriptions: Vec<Subscription>,
}

impl TasksView {
    pub fn new(
        backend: Backend,
        timezone: Tz,
        tokens: ThemeTokens,
        motion: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("タスクを追加"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("タスクを検索"));
        let edit_title = cx.new(|cx| InputState::new(window, cx).placeholder("タスク名"));
        let start_time = cx.new(|cx| InputState::new(window, cx).default_value("09:00"));
        let minutes = cx.new(|cx| InputState::new(window, cx).default_value("30"));
        let subscriptions =
            vec![
                cx.subscribe_in(&title, window, |this, input, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        let composing = input.update(cx, |input, cx| {
                            input.marked_text_range(window, cx).is_some()
                        });
                        if !composing {
                            this.add_task(window, cx);
                        }
                    }
                }),
            ];
        let owner = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.pending {
                        this.message = "処理中です。完了してからウィンドウを閉じてください".into();
                        cx.notify();
                        false
                    } else {
                        true
                    }
                })
                .unwrap_or(true)
        });
        let mut this = Self {
            backend,
            snapshot: None,
            title,
            search,
            edit_title,
            start_time,
            minutes,
            selected: None,
            date: Utc::now().with_timezone(&timezone).date_naive(),
            timezone,
            search_term: String::new(),
            page_offset: 0,
            pending: false,
            message: "読み込み中…".into(),
            is_error: false,
            tokens,
            motion,
            detail_epoch: 0,
            _subscriptions: subscriptions,
        };
        this.dispatch(Operation::Refresh, false, window, cx);
        this
    }

    fn dispatch(
        &mut self,
        operation: Operation,
        clear_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            return;
        }
        let refresh_editor = matches!(
            &operation,
            Operation::Undo | Operation::Redo | Operation::Execute(Command::UpdateTask { .. })
        );
        if matches!(&operation, Operation::Refresh) {
            // Never show the previous day's items under a newly selected date on read failure.
            self.snapshot = None;
        }
        self.pending = true;
        self.is_error = false;
        self.message = "処理中…".into();
        let receiver = self.backend.request(
            operation,
            ViewQuery {
                date: self.date,
                timezone: self.timezone,
                search: self.search_term.clone(),
                offset: self.page_offset,
            },
        );
        cx.spawn_in(window, async move |owner, cx| {
            let result = receiver.await.unwrap_or_else(|_| {
                Err("保存処理との接続が終了しました。アプリを再起動してください".into())
            });
            let _ = owner.update_in(cx, |this, window, cx| {
                this.pending = false;
                match result {
                    Ok(snapshot) => {
                        this.message = snapshot
                            .notification_warning
                            .clone()
                            .unwrap_or_else(|| "ローカルに保存済み".into())
                            .into();
                        this.is_error = snapshot.notification_warning.is_some();
                        this.snapshot = Some(snapshot);
                        if clear_input {
                            this.title
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        if let Some(task) = this.selected_task() {
                            if refresh_editor {
                                this.edit_title.update(cx, |input, cx| {
                                    input.set_value(task.title, window, cx)
                                });
                            }
                        } else {
                            this.selected = None;
                        }
                    }
                    Err(error) => {
                        this.is_error = true;
                        this.message = error.into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn selected_task(&self) -> Option<CoreTask> {
        self.snapshot
            .as_ref()?
            .tasks
            .iter()
            .find(|task| Some(task.id) == self.selected)
            .cloned()
    }

    fn add_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title.read(cx).value().trim().to_string();
        if title.is_empty() {
            return;
        }
        self.dispatch(
            Operation::Execute(Command::CreateTask {
                title,
                due: None,
                list_id: None,
            }),
            true,
            window,
            cx,
        );
    }

    fn select(&mut self, id: TaskId, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.selected = Some(id);
        self.detail_epoch += 1;
        if let Some(task) = self.selected_task() {
            self.edit_title
                .update(cx, |input, cx| input.set_value(task.title, window, cx));
        }
        cx.notify();
    }

    fn schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.selected_task() else {
            return;
        };
        match schedule_range(
            self.date,
            &self.start_time.read(cx).value(),
            &self.minutes.read(cx).value(),
            self.timezone,
        ) {
            Ok((start, end)) => self.dispatch(
                Operation::Execute(Command::ScheduleTask {
                    task_id: task.id,
                    expected_task_revision: task.revision,
                    start,
                    end,
                    timezone: self.timezone.to_string(),
                }),
                false,
                window,
                cx,
            ),
            Err(error) => {
                self.message = error.into();
                self.is_error = true;
                cx.notify();
            }
        }
    }

    fn move_block(&mut self, block: TimeBlock, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = block.start.checked_add_signed(Duration::minutes(30)) else {
            return;
        };
        let Some(end) = block.end.checked_add_signed(Duration::minutes(30)) else {
            return;
        };
        self.dispatch(
            Operation::Execute(Command::UpdateTimeBlock {
                id: block.id,
                expected_revision: block.revision,
                start,
                end,
                timezone: block.timezone,
            }),
            false,
            window,
            cx,
        );
    }

    fn change_day(&mut self, days: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if let Some(date) = self.date.checked_add_signed(Duration::days(days)) {
            self.date = date;
            self.dispatch(Operation::Refresh, false, window, cx);
        }
    }
}
