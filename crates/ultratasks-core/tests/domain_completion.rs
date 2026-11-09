use chrono::{DateTime, NaiveDate, Utc};
use ultratasks_core::*;
fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}
fn time(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}
fn query(start: &str, end: &str, timezone: &str) -> CalendarQuery {
    CalendarQuery {
        start: time(start),
        end: time(end),
        timezone: timezone.into(),
        limit: 1000,
    }
}
fn create_event(engine: &mut Engine, schedule: EventSchedule) -> Event {
    let Outcome::Event(value) = engine
        .execute(Command::CreateEvent {
            title: "予定".into(),
            schedule,
        })
        .unwrap()
    else {
        panic!()
    };
    value
}
fn create_task(engine: &mut Engine, due: DueDate) -> Task {
    let Outcome::Task(value) = engine
        .execute(Command::CreateTask {
            title: "作業".into(),
            due: Some(due),
            list_id: None,
        })
        .unwrap()
    else {
        panic!()
    };
    value
}
fn rule(frequency: Frequency, count: Option<u32>) -> Recurrence {
    Recurrence {
        frequency,
        interval: 1,
        count,
        until: None,
    }
}

#[test]
fn calendar_grouping_refuses_orphaning_and_roundtrips_undo() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Calendar(calendar) = engine
        .execute(Command::CreateCalendar {
            title: "個人".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    let event = create_event(
        &mut engine,
        EventSchedule::AllDay {
            start: date("2026-11-10"),
            end: date("2026-11-11"),
        },
    );
    engine
        .execute(Command::AssignEventCalendar {
            id: event.id,
            expected_revision: 1,
            calendar_id: Some(calendar.id),
        })
        .unwrap();
    assert!(
        engine
            .execute(Command::DeleteCalendar {
                id: calendar.id,
                expected_revision: 1
            })
            .is_err()
    );
    assert_eq!(
        engine
            .event_occurrences(
                &query("2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z", "UTC"),
                Some(calendar.id)
            )
            .unwrap()
            .len(),
        1
    );
    engine.undo().unwrap();
    assert_eq!(engine.event(event.id).unwrap().calendar_id, None);
    engine.redo().unwrap();
    assert_eq!(
        engine.event(event.id).unwrap().calendar_id,
        Some(calendar.id)
    );
}

#[test]
fn weekly_interval_uses_anchor_week_and_multiple_weekdays() {
    let mut engine = Engine::in_memory().unwrap();
    let event = create_event(
        &mut engine,
        EventSchedule::AllDay {
            start: date("2026-11-09"),
            end: date("2026-11-10"),
        },
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(Recurrence {
                frequency: Frequency::Weekly {
                    weekdays: vec![5, 1],
                },
                interval: 2,
                count: Some(4),
                until: None,
            }),
        })
        .unwrap();
    let dates: Vec<_> = engine
        .event_occurrences(
            &query("2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z", "UTC"),
            None,
        )
        .unwrap()
        .into_iter()
        .map(|v| v.original_date)
        .collect();
    assert_eq!(
        dates,
        vec![
            date("2026-11-09"),
            date("2026-11-13"),
            date("2026-11-23"),
            date("2026-11-27")
        ]
    );
}

#[test]
fn monthly_recurrence_skips_missing_days_and_counts_valid_dates() {
    let mut engine = Engine::in_memory().unwrap();
    let event = create_event(
        &mut engine,
        EventSchedule::AllDay {
            start: date("2028-01-31"),
            end: date("2028-02-01"),
        },
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Monthly, Some(3))),
        })
        .unwrap();
    let dates: Vec<_> = engine
        .event_occurrences(
            &query("2028-01-01T00:00:00Z", "2028-07-01T00:00:00Z", "UTC"),
            None,
        )
        .unwrap()
        .into_iter()
        .map(|v| v.original_date)
        .collect();
    assert_eq!(
        dates,
        vec![date("2028-01-31"), date("2028-03-31"), date("2028-05-31")]
    );
}

#[test]
fn dst_gap_does_not_consume_count_and_fold_is_earliest() {
    let mut engine = Engine::in_memory().unwrap();
    let event = create_event(
        &mut engine,
        EventSchedule::Timed {
            start: time("2026-03-07T07:30:00Z"),
            end: time("2026-03-07T08:00:00Z"),
            timezone: "America/New_York".into(),
        },
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Daily, Some(2))),
        })
        .unwrap();
    let values = engine
        .event_occurrences(
            &query(
                "2026-03-07T00:00:00Z",
                "2026-03-11T00:00:00Z",
                "America/New_York",
            ),
            None,
        )
        .unwrap();
    assert_eq!(
        values.iter().map(|v| v.original_date).collect::<Vec<_>>(),
        vec![date("2026-03-07"), date("2026-03-09")]
    );
    assert!(
        matches!(&values[1].schedule,EventSchedule::Timed{start,..} if *start==time("2026-03-09T06:30:00Z"))
    );
    let event = create_event(
        &mut engine,
        EventSchedule::Timed {
            start: time("2026-10-31T05:30:00Z"),
            end: time("2026-10-31T06:00:00Z"),
            timezone: "America/New_York".into(),
        },
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Daily, Some(2))),
        })
        .unwrap();
    let values = engine
        .event_occurrences(
            &query(
                "2026-11-01T00:00:00Z",
                "2026-11-02T00:00:00Z",
                "America/New_York",
            ),
            None,
        )
        .unwrap();
    assert!(
        matches!(&values[0].schedule,EventSchedule::Timed{start,..} if *start==time("2026-11-01T05:30:00Z"))
    );
}

#[test]
fn moved_exception_enters_query_from_far_original_date_without_duplicate() {
    let mut engine = Engine::in_memory().unwrap();
    let event = create_event(
        &mut engine,
        EventSchedule::AllDay {
            start: date("2026-01-01"),
            end: date("2026-01-02"),
        },
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Daily, Some(2))),
        })
        .unwrap();
    engine
        .execute(Command::SetEventException {
            id: event.id,
            expected_revision: 2,
            date: date("2026-01-02"),
            exception: Some(EventException::Override {
                title: Some("移動".into()),
                schedule: EventSchedule::AllDay {
                    start: date("2026-11-10"),
                    end: date("2026-11-11"),
                },
            }),
        })
        .unwrap();
    let values = engine
        .event_occurrences(
            &query("2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z", "UTC"),
            None,
        )
        .unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].original_date, date("2026-01-02"));
    assert_eq!(
        engine
            .event_occurrences(
                &query("2026-01-01T00:00:00Z", "2026-01-03T00:00:00Z", "UTC"),
                None
            )
            .unwrap()
            .len(),
        1
    );
    engine.undo().unwrap();
    assert_eq!(
        engine
            .event_occurrences(
                &query("2026-11-10T00:00:00Z", "2026-11-11T00:00:00Z", "UTC"),
                None
            )
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn recurring_task_completion_is_per_occurrence_and_preserves_series() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    engine
        .execute(Command::SetTaskRecurrence {
            id: task.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Daily, Some(3))),
        })
        .unwrap();
    engine
        .execute(Command::SetTaskException {
            id: task.id,
            expected_revision: 2,
            date: date("2026-11-10"),
            exception: Some(TaskException::Override {
                title: None,
                due: DueDate::Date(date("2026-11-10")),
                completed: true,
            }),
        })
        .unwrap();
    let values = engine
        .task_occurrences(&query(
            "2026-11-09T00:00:00Z",
            "2026-11-12T00:00:00Z",
            "UTC",
        ))
        .unwrap();
    assert_eq!(
        values.iter().map(|v| v.completed).collect::<Vec<_>>(),
        vec![false, true, false]
    );
    assert!(!engine.task(task.id).unwrap().completed);
    engine.undo().unwrap();
    assert!(
        !engine
            .task_occurrences(&query(
                "2026-11-10T00:00:00Z",
                "2026-11-11T00:00:00Z",
                "UTC"
            ))
            .unwrap()[0]
            .completed
    );
}

#[test]
fn invalid_exception_and_rule_change_leave_history_intact() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    engine
        .execute(Command::SetTaskRecurrence {
            id: task.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Daily, Some(2))),
        })
        .unwrap();
    assert!(
        engine
            .execute(Command::SetTaskException {
                id: task.id,
                expected_revision: 2,
                date: date("2026-11-11"),
                exception: Some(TaskException::Skip)
            })
            .is_err()
    );
    assert_eq!(engine.task(task.id).unwrap().revision, 2);
    engine
        .execute(Command::SetTaskException {
            id: task.id,
            expected_revision: 2,
            date: date("2026-11-10"),
            exception: Some(TaskException::Skip),
        })
        .unwrap();
    assert!(
        engine
            .execute(Command::SetTaskRecurrence {
                id: task.id,
                expected_revision: 3,
                recurrence: None
            })
            .is_err()
    );
}

#[test]
fn target_delete_cascades_reminders_and_undo_restores_them_atomically() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    let Outcome::Reminder(reminder) = engine
        .execute(Command::CreateReminder {
            target: EntityId::Task(task.id),
            trigger: ReminderTrigger::Before { minutes: 15 },
        })
        .unwrap()
    else {
        panic!()
    };
    engine
        .execute(Command::DeleteTask {
            id: task.id,
            expected_revision: 1,
        })
        .unwrap();
    assert!(engine.reminder(reminder.id).is_err());
    assert_eq!(engine.reminders(100).unwrap().len(), 0);
    engine.undo().unwrap();
    assert!(engine.reminder(reminder.id).is_ok());
    assert!(engine.task(task.id).is_ok());
    engine.redo().unwrap();
    assert!(engine.reminder(reminder.id).is_err());
}

#[test]
fn due_filter_respects_local_day_and_paginates_after_filtering() {
    let mut engine = Engine::in_memory().unwrap();
    create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    create_task(&mut engine, DueDate::Date(date("2026-11-10")));
    create_task(
        &mut engine,
        DueDate::At {
            at: time("2026-11-09T23:00:00Z"),
            timezone: "Asia/Tokyo".into(),
        },
    );
    let values = engine
        .tasks_due(
            &TaskQuery::default(),
            &DueFilter::Before {
                at: time("2026-11-10T00:00:00Z"),
                timezone: "Asia/Tokyo".into(),
            },
        )
        .unwrap();
    assert_eq!(values.len(), 2);
    let values = engine
        .tasks_due(
            &TaskQuery {
                limit: 1,
                offset: 1,
                ..Default::default()
            },
            &DueFilter::Before {
                at: time("2026-11-10T00:00:00Z"),
                timezone: "Asia/Tokyo".into(),
            },
        )
        .unwrap();
    assert_eq!(values.len(), 1);
    assert!(matches!(values[0].due, Some(DueDate::At { .. })));
}

#[test]
fn request_guards_are_atomic_and_replay_precedes_guard_check() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    let command = Command::SetTaskCompleted {
        id: task.id,
        expected_revision: 1,
        completed: true,
    };
    engine
        .execute_idempotent_guarded(
            "proposal",
            vec![command.clone()],
            &[(EntityId::Task(task.id), 1)],
        )
        .unwrap();
    engine
        .execute_idempotent_guarded("proposal", vec![command], &[(EntityId::Task(task.id), 1)])
        .unwrap();
    assert_eq!(engine.task(task.id).unwrap().revision, 2);
    assert!(
        engine
            .execute_idempotent_guarded(
                "different",
                vec![Command::CreateList {
                    title: "should not exist".into()
                }],
                &[(EntityId::Task(task.id), 1)]
            )
            .is_err()
    );
    assert!(engine.lists(100).unwrap().is_empty());
}

#[test]
fn preview_leaves_records_history_and_sync_outbox_unchanged() {
    let mut engine = Engine::in_memory().unwrap();
    engine
        .preview_batch(vec![Command::CreateTask {
            title: "preview".into(),
            due: None,
            list_id: None,
        }])
        .unwrap();
    assert!(engine.snapshots().unwrap().is_empty());
    assert!(!engine.history_status().unwrap().can_undo);
}

#[test]
fn scoped_request_cannot_delete_a_new_unselected_dependent_entity() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    let commands = vec![Command::DeleteTask {
        id: task.id,
        expected_revision: 1,
    }];
    let guards = vec![(EntityId::Task(task.id), 1)];
    engine
        .preview_batch_scoped(commands.clone(), &guards)
        .unwrap();
    let Outcome::Reminder(reminder) = engine
        .execute(Command::CreateReminder {
            target: EntityId::Task(task.id),
            trigger: ReminderTrigger::Before { minutes: 10 },
        })
        .unwrap()
    else {
        panic!()
    };
    assert!(
        engine
            .execute_idempotent_scoped("reviewed", commands, &guards)
            .is_err()
    );
    assert!(engine.task(task.id).is_ok());
    assert!(engine.reminder(reminder.id).is_ok());
}

#[test]
fn recurrence_respects_series_timezone_across_international_date_line() {
    let mut engine = Engine::in_memory().unwrap();
    let event = create_event(
        &mut engine,
        EventSchedule::Timed {
            start: time("2026-11-08T11:00:00Z"),
            end: time("2026-11-08T11:30:00Z"),
            timezone: "Pacific/Kiritimati".into(),
        },
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(rule(Frequency::Daily, None)),
        })
        .unwrap();
    // Nov 9 in UTC-12 includes Nov 11 01:00 in UTC+14.
    let query = query("2026-11-09T12:00:00Z", "2026-11-10T12:00:00Z", "Etc/GMT+12");
    let occurrences = engine.event_occurrences(&query, None).unwrap();
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].original_date, date("2026-11-11"));
}

#[test]
fn read_only_open_never_creates_or_migrates_and_sqlite_rejects_writes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("readonly.sqlite3");
    assert!(Engine::open_read_only(&path).is_err());
    assert!(!path.exists());
    let mut engine = Engine::open(&path).unwrap();
    let task = create_task(&mut engine, DueDate::Date(date("2026-11-09")));
    drop(engine);
    let mut readonly = Engine::open_read_only(&path).unwrap();
    assert_eq!(readonly.task(task.id).unwrap(), task);
    assert!(
        readonly
            .execute(Command::CreateList {
                title: "blocked".into()
            })
            .is_err()
    );
    drop(readonly);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 2).unwrap();
    assert!(matches!(
        Engine::open_read_only(&path),
        Err(Error::UnsupportedSchema(2))
    ));
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
}
