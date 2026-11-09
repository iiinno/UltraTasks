use chrono::{DateTime, Utc};
use ultratasks_core::*;

fn instant(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}
fn task(engine: &mut Engine, title: &str) -> Task {
    match engine
        .execute(Command::CreateTask {
            title: title.into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    {
        Outcome::Task(value) => value,
        _ => unreachable!(),
    }
}
fn event(engine: &mut Engine) -> Event {
    match engine
        .execute(Command::CreateEvent {
            title: "予定、日本語; \\".into(),
            schedule: EventSchedule::Timed {
                start: instant("2026-03-07T14:00:00Z"),
                end: instant("2026-03-07T15:00:00Z"),
                timezone: "America/New_York".into(),
            },
        })
        .unwrap()
    {
        Outcome::Event(value) => value,
        _ => unreachable!(),
    }
}

#[test]
fn complete_json_roundtrip_preserves_relations_recurrence_and_tombstones() {
    let mut source = Engine::in_memory().unwrap();
    let list = match source
        .execute(Command::CreateList {
            title: "仕事".into(),
        })
        .unwrap()
    {
        Outcome::List(v) => v,
        _ => unreachable!(),
    };
    let task = match source
        .execute(Command::CreateTask {
            title: "日課".into(),
            due: Some(DueDate::Date("2026-01-31".parse().unwrap())),
            list_id: Some(list.id),
        })
        .unwrap()
    {
        Outcome::Task(v) => v,
        _ => unreachable!(),
    };
    source
        .execute(Command::SetTaskRecurrence {
            id: task.id,
            expected_revision: task.revision,
            recurrence: Some(Recurrence {
                frequency: Frequency::Monthly,
                interval: 1,
                count: Some(6),
                until: None,
            }),
        })
        .unwrap();
    let task = source.task(task.id).unwrap();
    source
        .execute(Command::SetTaskException {
            id: task.id,
            expected_revision: task.revision,
            date: "2026-03-31".parse().unwrap(),
            exception: Some(TaskException::Override {
                title: Some("例外".into()),
                due: DueDate::Date("2026-04-01".parse().unwrap()),
                completed: true,
            }),
        })
        .unwrap();
    let task = source.task(task.id).unwrap();
    source
        .execute(Command::ScheduleTask {
            task_id: task.id,
            expected_task_revision: task.revision,
            start: instant("2026-11-09T01:00:00Z"),
            end: instant("2026-11-09T02:00:00Z"),
            timezone: "Asia/Tokyo".into(),
        })
        .unwrap();
    let calendar = match source
        .execute(Command::CreateCalendar {
            title: "個人".into(),
        })
        .unwrap()
    {
        Outcome::Calendar(v) => v,
        _ => unreachable!(),
    };
    let event = event(&mut source);
    source
        .execute(Command::AssignEventCalendar {
            id: event.id,
            expected_revision: event.revision,
            calendar_id: Some(calendar.id),
        })
        .unwrap();
    source
        .execute(Command::CreateReminder {
            target: EntityId::Event(event.id),
            trigger: ReminderTrigger::Before { minutes: 10 },
        })
        .unwrap();
    let deleted = super_task(&mut source, "deleted");
    source
        .execute(Command::DeleteTask {
            id: deleted.id,
            expected_revision: deleted.revision,
        })
        .unwrap();
    let json = source.export_json().unwrap();
    let mut target = Engine::in_memory().unwrap();
    let report = target.import_json(&json, ImportMode::Empty).unwrap();
    assert_eq!(report.inserted, 7);
    assert_eq!(source.export_json().unwrap(), target.export_json().unwrap());
    assert_eq!(
        target.import_json(&json, ImportMode::Merge).unwrap(),
        ImportReport {
            inserted: 0,
            unchanged: 7
        }
    );
    assert!(target.import_json(&json, ImportMode::Empty).is_err());
    target.undo().unwrap();
    assert!(target.tasks(&TaskQuery::default()).unwrap().is_empty());
    target.redo().unwrap();
    assert_eq!(target.task(task.id).unwrap().exceptions, task.exceptions);
    assert_eq!(
        target
            .time_blocks(&CalendarQuery {
                start: instant("2026-11-09T00:00:00Z"),
                end: instant("2026-11-10T00:00:00Z"),
                timezone: "UTC".into(),
                limit: 100
            })
            .unwrap()
            .len(),
        1
    );
}
fn super_task(engine: &mut Engine, title: &str) -> Task {
    task(engine, title)
}

#[test]
fn invalid_mixed_import_collisions_unknown_fields_and_duplicates_are_atomic() {
    let mut source = Engine::in_memory().unwrap();
    task(&mut source, "original");
    task(&mut source, "second");
    let json = source.export_json().unwrap();
    let baseline: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut target = Engine::in_memory().unwrap();
    for change in [0, 1, 2, 3] {
        let mut corrupt = baseline.clone();
        match change {
            0 => corrupt["records"][1]["entity"]["Task"]["title"] = " ".into(),
            1 => {
                let first = corrupt["records"][0].clone();
                corrupt["records"].as_array_mut().unwrap().push(first);
            }
            2 => {
                corrupt["records"][0]["entity"]["Task"]["future_field"] =
                    "not silently discarded".into()
            }
            _ => corrupt["records"][0]["entity"]["Task"]["revision"] = 0.into(),
        }
        assert!(
            target
                .import_json(&corrupt.to_string(), ImportMode::Merge)
                .is_err()
        );
        assert!(target.snapshots().unwrap().is_empty());
    }
    target.import_json(&json, ImportMode::Empty).unwrap();
    let mut collision = baseline;
    collision["records"][0]["entity"]["Task"]["title"] = "overwritten?".into();
    assert!(
        target
            .import_json(&collision.to_string(), ImportMode::Merge)
            .is_err()
    );
    assert_eq!(target.export_json().unwrap(), json);
}

#[test]
fn online_backup_contains_committed_wal_and_history_restore_never_clobbers() {
    let directory = tempfile::tempdir().unwrap();
    let live = directory.path().join("live.sqlite3");
    let backup = directory.path().join("backup.sqlite3");
    let restored = directory.path().join("restored.sqlite3");
    let mut source = Engine::open(&live).unwrap();
    let task = task(&mut source, "WALに保存されたタスク");
    source
        .execute(Command::SetTaskCompleted {
            id: task.id,
            expected_revision: task.revision,
            completed: true,
        })
        .unwrap();
    source.backup_to(&backup).unwrap();
    assert!(source.backup_to(&backup).is_err());
    Engine::restore_backup(&backup, &restored).unwrap();
    let mut restored_engine = Engine::open(&restored).unwrap();
    assert!(restored_engine.task(task.id).unwrap().completed);
    restored_engine.undo().unwrap();
    assert!(!restored_engine.task(task.id).unwrap().completed);
    assert!(source.task(task.id).unwrap().completed);
    let foreign = directory.path().join("foreign.sqlite3");
    std::fs::write(&foreign, b"do not overwrite").unwrap();
    assert!(source.backup_to(&foreign).is_err());
    assert!(Engine::restore_backup(&backup, &foreign).is_err());
    assert_eq!(std::fs::read(&foreign).unwrap(), b"do not overwrite");
    let bad_target = directory.path().join("bad-target.sqlite3");
    assert!(Engine::restore_backup(&foreign, &bad_target).is_err());
    assert!(!bad_target.exists());
}

#[test]
fn restore_rejects_foreign_and_newer_schema_without_changing_source() {
    let directory = tempfile::tempdir().unwrap();
    let foreign = directory.path().join("foreign.sqlite3");
    let db = rusqlite::Connection::open(&foreign).unwrap();
    db.execute_batch(
        "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('important');",
    )
    .unwrap();
    drop(db);
    let before = std::fs::read(&foreign).unwrap();
    assert!(Engine::restore_backup(&foreign, directory.path().join("target")).is_err());
    assert_eq!(std::fs::read(&foreign).unwrap(), before);
    let future = directory.path().join("future.sqlite3");
    drop(Engine::open(&future).unwrap());
    let db = rusqlite::Connection::open(&future).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    assert!(Engine::restore_backup(&future, directory.path().join("future-target")).is_err());
    let empty = directory.path().join("empty.sqlite3");
    drop(rusqlite::Connection::open(&empty).unwrap());
    assert!(Engine::restore_backup(&empty, directory.path().join("empty-target")).is_err());
}

const RECURRING: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Tests//EN\r\nBEGIN:VEVENT\r\nUID:recurring@example.test\r\nSUMMARY:朝会\\,日本語\\;確認\\\\\r\nDTSTART;TZID=America/New_York:20260307T090000\r\nDTEND;TZID=America/New_York:20260307T100000\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE;TZID=America/New_York:20260309T090000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:recurring@example.test\r\nRECURRENCE-ID;TZID=America/New_York:20260310T090000\r\nSUMMARY:移動\r\nDTSTART;TZID=America/New_York:20260310T110000\r\nDTEND;TZID=America/New_York:20260310T120000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

#[test]
fn ics_recurring_dst_exclusion_override_roundtrip_and_idempotence() {
    let mut engine = Engine::in_memory().unwrap();
    assert_eq!(
        engine
            .import_ics(RECURRING, ImportMode::Empty)
            .unwrap()
            .inserted,
        1
    );
    assert_eq!(
        engine
            .import_ics(RECURRING, ImportMode::Merge)
            .unwrap()
            .unchanged,
        1
    );
    let query = CalendarQuery {
        start: instant("2026-03-07T00:00:00Z"),
        end: instant("2026-03-12T00:00:00Z"),
        timezone: "America/New_York".into(),
        limit: 100,
    };
    let occurrences = engine.event_occurrences(&query, None).unwrap();
    assert_eq!(occurrences.len(), 3);
    let starts: Vec<_> = occurrences
        .iter()
        .map(|occurrence| match occurrence.schedule {
            EventSchedule::Timed { start, .. } => start,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        starts,
        vec![
            instant("2026-03-07T14:00:00Z"),
            instant("2026-03-08T13:00:00Z"),
            instant("2026-03-10T15:00:00Z")
        ]
    );
    let export = engine.export_ics().unwrap();
    assert!(export.notices.is_empty());
    assert!(export.text.split("\r\n").all(|line| line.len() <= 75));
    let mut restored = Engine::in_memory().unwrap();
    restored
        .import_ics(&export.text, ImportMode::Empty)
        .unwrap();
    assert_eq!(
        restored.event_occurrences(&query, None).unwrap(),
        occurrences
    );
}

#[test]
fn ics_utf8_folding_all_day_and_tasks_roundtrip() {
    let mut engine = Engine::in_memory().unwrap();
    let title = "日本語の長い予定、".repeat(20);
    engine
        .execute(Command::CreateEvent {
            title: title.clone(),
            schedule: EventSchedule::AllDay {
                start: "2028-02-29".parse().unwrap(),
                end: "2028-03-02".parse().unwrap(),
            },
        })
        .unwrap();
    engine
        .execute(Command::CreateTask {
            title: "買う, もの;\\".into(),
            due: Some(DueDate::Date("2028-02-29".parse().unwrap())),
            list_id: None,
        })
        .unwrap();
    let export = engine.export_ics().unwrap();
    assert!(export.text.contains("\r\n "));
    let mut restored = Engine::in_memory().unwrap();
    restored
        .import_ics(&export.text, ImportMode::Empty)
        .unwrap();
    assert_eq!(
        engine.export_json().unwrap(),
        restored.export_json().unwrap()
    );
}

#[test]
fn unsupported_ics_semantics_and_precision_fail_without_partial_import() {
    let mut engine = Engine::in_memory().unwrap();
    for input in [
        RECURRING.replace("FREQ=DAILY", "FREQ=YEARLY"),
        RECURRING.replace("SUMMARY:移動", "DESCRIPTION:cannot be lost\r\nSUMMARY:移動"),
        RECURRING.replace(
            "RECURRENCE-ID;TZID=America/New_York:",
            "RECURRENCE-ID;RANGE=THISANDFUTURE;TZID=America/New_York:",
        ),
        RECURRING.replace("TZID=America/New_York:", ":"),
        RECURRING.replace("20260307T090000", "20260308T023000"),
    ] {
        assert!(engine.import_ics(&input, ImportMode::Merge).is_err());
        assert!(engine.snapshots().unwrap().is_empty());
    }
    engine
        .execute(Command::CreateEvent {
            title: "fractional".into(),
            schedule: EventSchedule::Timed {
                start: instant("2026-01-01T00:00:00.001Z"),
                end: instant("2026-01-01T01:00:00Z"),
                timezone: "UTC".into(),
            },
        })
        .unwrap();
    assert!(engine.export_ics().is_err());
}

#[test]
fn ics_until_during_dst_fold_preserves_exact_generated_occurrence_set() {
    let input = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\nBEGIN:VEVENT\r\nUID:fold@example.test\r\nSUMMARY:Fold\r\nDTSTART;TZID=America/New_York:20261031T013000\r\nDTEND;TZID=America/New_York:20261031T020000\r\nRRULE:FREQ=DAILY;UNTIL=20261101T061500Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut source = Engine::in_memory().unwrap();
    source.import_ics(input, ImportMode::Empty).unwrap();
    let query = CalendarQuery {
        start: instant("2026-10-31T00:00:00Z"),
        end: instant("2026-11-03T00:00:00Z"),
        timezone: "America/New_York".into(),
        limit: 100,
    };
    let expected = source.event_occurrences(&query, None).unwrap();
    assert_eq!(expected.len(), 2);
    let export = source.export_ics().unwrap();
    let mut restored = Engine::in_memory().unwrap();
    restored
        .import_ics(&export.text, ImportMode::Empty)
        .unwrap();
    assert_eq!(restored.event_occurrences(&query, None).unwrap(), expected);
    let mut second_fold = Engine::in_memory().unwrap();
    second_fold
        .execute(Command::CreateEvent {
            title: "second fold".into(),
            schedule: EventSchedule::Timed {
                start: instant("2026-11-01T06:30:00Z"),
                end: instant("2026-11-01T07:30:00Z"),
                timezone: "America/New_York".into(),
            },
        })
        .unwrap();
    assert!(second_fold.export_ics().is_err());
}
