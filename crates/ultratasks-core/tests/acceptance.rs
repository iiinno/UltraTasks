//! User-visible acceptance cases, deliberately independent of the storage schema.
use chrono::{DateTime, NaiveDate, Utc};
use proptest::prelude::*;
use ultratasks_core::*;

fn instant(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}

fn create_task(engine: &mut Engine, title: &str) -> Task {
    match engine
        .execute(Command::CreateTask {
            title: title.into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    {
        Outcome::Task(task) => task,
        other => panic!("unexpected create result: {other:?}"),
    }
}

fn schedule(engine: &mut Engine, task: TaskId, start: &str, end: &str) -> TimeBlock {
    let current = engine.task(task).unwrap();
    match engine
        .execute(Command::ScheduleTask {
            task_id: task,
            expected_task_revision: current.revision,
            start: instant(start),
            end: instant(end),
            timezone: "Asia/Tokyo".into(),
        })
        .unwrap()
    {
        Outcome::TimeBlock(block) => block,
        other => panic!("unexpected schedule result: {other:?}"),
    }
}

fn rename(engine: &mut Engine, task: &Task, title: &str) -> Task {
    match engine
        .execute(Command::UpdateTask {
            id: task.id,
            expected_revision: task.revision,
            title: title.into(),
            completed: task.completed,
            due: task.due.clone(),
            list_id: task.list_id,
        })
        .unwrap()
    {
        Outcome::Task(task) => task,
        other => panic!("unexpected update result: {other:?}"),
    }
}

fn window(start: &str, end: &str, timezone: &str) -> CalendarQuery {
    CalendarQuery {
        start: instant(start),
        end: instant(end),
        timezone: timezone.into(),
        limit: 100,
    }
}

#[test]
fn task_schedule_completion_undo_and_redo_survive_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite3");
    let (task, block) = {
        let mut engine = Engine::open(&path).unwrap();
        let task = create_task(&mut engine, "日本語入力を確認する");
        let block = schedule(
            &mut engine,
            task.id,
            "2026-11-09T01:00:00Z",
            "2026-11-09T01:45:00Z",
        );
        let current = engine.task(task.id).unwrap();
        engine
            .execute(Command::SetTaskCompleted {
                id: task.id,
                expected_revision: current.revision,
                completed: true,
            })
            .unwrap();
        assert!(engine.task(task.id).unwrap().completed);
        engine.undo().unwrap();
        assert!(!engine.task(task.id).unwrap().completed);
        (task, block)
    };
    let mut reopened = Engine::open(&path).unwrap();
    let restored = reopened.task(task.id).unwrap();
    assert_eq!(restored.title, task.title);
    assert!(!restored.completed);
    assert_eq!(reopened.time_block(block.id).unwrap(), block);
    assert!(reopened.history_status().unwrap().can_redo);
    reopened.redo().unwrap();
    drop(reopened);
    let reopened = Engine::open(&path).unwrap();
    assert!(reopened.task(task.id).unwrap().completed);
    assert_eq!(reopened.time_block(block.id).unwrap().task_id, task.id);
}

#[test]
fn undo_cannot_overwrite_a_second_actors_edit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shared.sqlite3");
    let mut first = Engine::open_for_actor(&path, "desktop").unwrap();
    let mut second = Engine::open_for_actor(&path, "mobile").unwrap();
    let task = create_task(&mut first, "original");
    let task = rename(&mut first, &task, "desktop edit");
    let remote = rename(&mut second, &task, "mobile edit");
    assert!(first.undo().is_err());
    assert_eq!(first.task(task.id).unwrap(), remote);
    assert_eq!(second.task(task.id).unwrap(), remote);
}

#[test]
fn older_undo_stays_blocked_after_a_foreign_same_value_edit_and_a_new_local_edit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history-lineage.sqlite3");
    let mut first = Engine::open_for_actor(&path, "desktop").unwrap();
    let mut second = Engine::open_for_actor(&path, "mobile").unwrap();
    let task = create_task(&mut first, "original");
    let local = rename(&mut first, &task, "A");
    // A later writer reasserts the same text, but with a new update revision.
    let remote = rename(&mut second, &local, "A");
    assert!(remote.revision > local.revision);
    rename(&mut first, &remote, "C");
    first.undo().unwrap();
    let restored_remote = first.task(task.id).unwrap();
    assert_eq!(restored_remote.title, "A");
    // Undoing our latest write must not grant authority over the earlier remote one.
    assert!(first.undo().is_err());
    assert_eq!(first.task(task.id).unwrap(), restored_remote);
}

#[test]
fn undo_task_creation_cannot_orphan_another_actors_new_work_block() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dependent-history.sqlite3");
    let mut first = Engine::open_for_actor(&path, "desktop").unwrap();
    let mut second = Engine::open_for_actor(&path, "mobile").unwrap();
    let task = create_task(&mut first, "schedule from another device");
    let block = schedule(
        &mut second,
        task.id,
        "2026-11-09T01:00:00Z",
        "2026-11-09T02:00:00Z",
    );
    assert!(first.undo().is_err());
    assert_eq!(first.task(task.id).unwrap(), task);
    assert_eq!(first.time_block(block.id).unwrap(), block);
}

#[test]
fn date_only_and_instant_deadlines_keep_their_distinct_meanings_after_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("deadlines.sqlite3");
    let due_values = [
        DueDate::Date(date("2026-11-09")),
        DueDate::At {
            at: instant("2026-11-09T00:00:00Z"),
            timezone: "Asia/Tokyo".into(),
        },
    ];
    let mut ids = Vec::new();
    {
        let mut engine = Engine::open(&path).unwrap();
        for due in &due_values {
            let Outcome::Task(task) = engine
                .execute(Command::CreateTask {
                    title: "締切の意味を保持する".into(),
                    due: Some(due.clone()),
                    list_id: None,
                })
                .unwrap()
            else {
                panic!("expected task")
            };
            ids.push(task.id);
        }
    }
    let engine = Engine::open(&path).unwrap();
    for (id, due) in ids.into_iter().zip(due_values) {
        assert_eq!(engine.task(id).unwrap().due, Some(due));
    }
}

#[test]
fn monotonic_revisions_prevent_aba_after_undo_and_redo() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, "before");
    let updated = rename(&mut engine, &task, "after");
    engine.undo().unwrap();
    let undone = engine.task(task.id).unwrap();
    assert_eq!(undone.title, "before");
    assert!(undone.revision > updated.revision);
    assert!(
        engine
            .execute(Command::SetTaskCompleted {
                id: task.id,
                expected_revision: task.revision,
                completed: true,
            })
            .is_err()
    );
    assert_eq!(engine.task(task.id).unwrap(), undone);
    engine.redo().unwrap();
    let redone = engine.task(task.id).unwrap();
    assert_eq!(redone.title, "after");
    assert!(redone.revision > undone.revision);
    assert!(
        engine
            .execute(Command::DeleteTask {
                id: task.id,
                expected_revision: updated.revision,
            })
            .is_err()
    );
    assert_eq!(engine.task(task.id).unwrap(), redone);
}

#[test]
fn deleting_task_and_undo_preserve_its_linked_work_blocks_atomically() {
    let mut engine = Engine::in_memory().unwrap();
    let task = create_task(&mut engine, "one task, two work sessions");
    let first = schedule(
        &mut engine,
        task.id,
        "2026-11-09T01:00:00Z",
        "2026-11-09T02:00:00Z",
    );
    let second = schedule(
        &mut engine,
        task.id,
        "2026-11-09T03:00:00Z",
        "2026-11-09T04:00:00Z",
    );
    let current = engine.task(task.id).unwrap();
    engine
        .execute(Command::DeleteTask {
            id: task.id,
            expected_revision: current.revision,
        })
        .unwrap();
    assert!(engine.task(task.id).is_err());
    assert!(engine.time_block(first.id).is_err());
    assert!(engine.time_block(second.id).is_err());
    engine.undo().unwrap();
    assert_eq!(engine.task(task.id).unwrap().title, task.title);
    for original in [first, second] {
        let restored = engine.time_block(original.id).unwrap();
        assert_eq!(restored.task_id, task.id);
        assert_eq!(
            (restored.start, restored.end),
            (original.start, original.end)
        );
        assert!(restored.revision > original.revision);
    }
}

#[test]
fn sqlite_failure_after_a_parent_write_rolls_back_data_and_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("transaction-rollback.sqlite3");
    let mut engine = Engine::open(&path).unwrap();
    let task = create_task(&mut engine, "atomic delete");
    let block = schedule(
        &mut engine,
        task.id,
        "2026-11-09T01:00:00Z",
        "2026-11-09T02:00:00Z",
    );
    // Inject a disk-layer failure on the child write after the task update has begun.
    // This tests real SQLite rollback, rather than merely rejecting an invalid command.
    let fault = rusqlite::Connection::open(&path).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER block_delete_abort BEFORE UPDATE OF deleted ON time_blocks
        WHEN NEW.deleted = 1 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    let before = engine.task(task.id).unwrap();
    assert!(
        engine
            .execute(Command::DeleteTask {
                id: task.id,
                expected_revision: before.revision
            })
            .is_err()
    );
    assert_eq!(engine.task(task.id).unwrap(), before);
    assert_eq!(engine.time_block(block.id).unwrap(), block);
    fault
        .execute_batch("DROP TRIGGER block_delete_abort;")
        .unwrap();
    // The failed deletion must not have become the latest undoable operation.
    engine.undo().unwrap();
    assert!(engine.time_block(block.id).is_err());
    assert_eq!(engine.task(task.id).unwrap(), before);
}

#[test]
fn list_deletion_with_members_is_rejected_and_does_not_change_history() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::List(list) = engine
        .execute(Command::CreateList {
            title: "仕事".into(),
        })
        .unwrap()
    else {
        panic!("expected list");
    };
    let Outcome::Task(task) = engine
        .execute(Command::CreateTask {
            title: "提出する".into(),
            due: None,
            list_id: Some(list.id),
        })
        .unwrap()
    else {
        panic!("expected task");
    };
    assert!(
        engine
            .execute(Command::DeleteList {
                id: list.id,
                expected_revision: list.revision,
            })
            .is_err()
    );
    assert_eq!(engine.task(task.id).unwrap().list_id, Some(list.id));
    engine.undo().unwrap();
    assert!(engine.task(task.id).is_err());
    assert_eq!(engine.task_list(list.id).unwrap(), list);
}

#[test]
fn missing_references_are_rejected_without_orphans() {
    let mut engine = Engine::in_memory().unwrap();
    assert!(
        engine
            .execute(Command::CreateTask {
                title: "no list".into(),
                due: None,
                list_id: Some(ListId::new()),
            })
            .is_err()
    );
    assert!(
        engine
            .execute(Command::ScheduleTask {
                task_id: TaskId::new(),
                expected_task_revision: 1,
                start: instant("2026-11-09T01:00:00Z"),
                end: instant("2026-11-09T02:00:00Z"),
                timezone: "Asia/Tokyo".into(),
            })
            .is_err()
    );
    assert!(engine.tasks(&TaskQuery::default()).unwrap().is_empty());
    assert!(
        engine
            .time_blocks(&window(
                "2026-11-09T00:00:00Z",
                "2026-11-10T00:00:00Z",
                "Asia/Tokyo"
            ))
            .unwrap()
            .is_empty()
    );
    assert!(!engine.history_status().unwrap().can_undo);
}

#[test]
fn japanese_search_treats_sql_wildcards_as_literal_characters() {
    let mut engine = Engine::in_memory().unwrap();
    let literal = create_task(&mut engine, "仕様書_100%\\確認's");
    create_task(&mut engine, "仕様書A100X確認");
    create_task(&mut engine, "確認資料");
    for search in ["仕様書_", "100%", "\\", "'s"] {
        let results = engine
            .tasks(&TaskQuery {
                search: Some(search.into()),
                ..TaskQuery::default()
            })
            .unwrap();
        assert_eq!(results.len(), 1, "query: {search}");
        assert_eq!(results[0].id, literal.id);
    }
    assert_eq!(
        engine
            .tasks(&TaskQuery {
                search: Some("確認".into()),
                ..TaskQuery::default()
            })
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn invalid_dates_and_timezones_never_leave_partial_events() {
    let mut engine = Engine::in_memory().unwrap();
    let invalid = [
        EventSchedule::AllDay {
            start: date("2026-11-09"),
            end: date("2026-11-09"),
        },
        EventSchedule::Timed {
            start: instant("2026-11-09T02:00:00Z"),
            end: instant("2026-11-09T01:00:00Z"),
            timezone: "Asia/Tokyo".into(),
        },
        EventSchedule::Timed {
            start: instant("2026-11-09T01:00:00Z"),
            end: instant("2026-11-09T02:00:00Z"),
            timezone: "Not/AZone".into(),
        },
    ];
    for schedule in invalid {
        assert!(
            engine
                .execute(Command::CreateEvent {
                    title: "invalid".into(),
                    schedule
                })
                .is_err()
        );
    }
    assert!(
        engine
            .events(&window(
                "2026-11-08T00:00:00Z",
                "2026-11-11T00:00:00Z",
                "UTC"
            ))
            .unwrap()
            .is_empty()
    );
    assert!(!engine.history_status().unwrap().can_undo);
}

#[test]
fn all_day_dates_and_timed_overlap_respect_a_25_hour_dst_day() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Event(all_day) = engine
        .execute(Command::CreateEvent {
            title: "終日・時計の切替日".into(),
            schedule: EventSchedule::AllDay {
                start: date("2026-11-01"),
                end: date("2026-11-02"),
            },
        })
        .unwrap()
    else {
        panic!("expected event")
    };
    let Outcome::Event(crosses_midnight) = engine
        .execute(Command::CreateEvent {
            title: "日跨ぎ".into(),
            schedule: EventSchedule::Timed {
                start: instant("2026-11-02T04:30:00Z"),
                end: instant("2026-11-02T05:30:00Z"),
                timezone: "America/New_York".into(),
            },
        })
        .unwrap()
    else {
        panic!("expected event")
    };
    let Outcome::Event(ends_at_midnight) = engine
        .execute(Command::CreateEvent {
            title: "終了境界".into(),
            schedule: EventSchedule::Timed {
                start: instant("2026-11-02T04:00:00Z"),
                end: instant("2026-11-02T05:00:00Z"),
                timezone: "America/New_York".into(),
            },
        })
        .unwrap()
    else {
        panic!("expected event")
    };
    let first = engine
        .events(&window(
            "2026-11-01T04:00:00Z",
            "2026-11-02T05:00:00Z",
            "America/New_York",
        ))
        .unwrap();
    assert_eq!(first.len(), 3);
    let next = engine
        .events(&window(
            "2026-11-02T05:00:00Z",
            "2026-11-03T05:00:00Z",
            "America/New_York",
        ))
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].id, crosses_midnight.id);
    assert!(
        !next
            .iter()
            .any(|event| event.id == all_day.id || event.id == ends_at_midnight.id)
    );
    assert_eq!(engine.event(all_day.id).unwrap().schedule, all_day.schedule);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn task_edit_history_matches_a_small_reference_model(operations in prop::collection::vec((0u8..4, any::<u16>()), 1..80)) {
        let mut engine = Engine::in_memory().unwrap();
        let task = create_task(&mut engine, "initial");
        let mut expected = (String::from("initial"), false);
        let mut past = Vec::new();
        let mut future = Vec::new();
        let mut previous_revision = task.revision;
        for (index, (operation, payload)) in operations.into_iter().enumerate() {
            let current = engine.task(task.id).unwrap();
            let changed = match operation {
                0 => {
                    past.push(expected.clone());
                    future.clear();
                    expected.1 = !expected.1;
                    engine.execute(Command::SetTaskCompleted { id: task.id, expected_revision: current.revision, completed: expected.1 }).unwrap();
                    true
                }
                1 => {
                    past.push(expected.clone());
                    future.clear();
                    expected.0 = format!("編集 {index} / {payload}");
                    rename(&mut engine, &current, &expected.0);
                    true
                }
                2 if !past.is_empty() => {
                    future.push(expected.clone());
                    expected = past.pop().unwrap();
                    engine.undo().unwrap();
                    true
                }
                3 if !future.is_empty() => {
                    past.push(expected.clone());
                    expected = future.pop().unwrap();
                    engine.redo().unwrap();
                    true
                }
                _ => false,
            };
            let actual = engine.task(task.id).unwrap();
            prop_assert_eq!((&actual.title, actual.completed), (&expected.0, expected.1));
            if changed { prop_assert!(actual.revision > previous_revision); }
            previous_revision = actual.revision;
        }
    }
}
