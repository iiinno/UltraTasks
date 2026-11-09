//! Independent failure scenarios across the domain, durable history and transfer APIs.
use chrono::{DateTime, NaiveDate, Utc};
use ultratasks_core::{
    CalendarQuery, Command, DueDate, Engine, EntityId, EventException, EventSchedule, Frequency,
    ImportMode, Outcome, Recurrence, ReminderTrigger, TaskException, TaskQuery,
};

fn utc(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}
fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}
fn create_task(title: &str) -> Command {
    Command::CreateTask {
        title: title.into(),
        due: None,
        list_id: None,
    }
}
fn query(start: &str, end: &str) -> CalendarQuery {
    CalendarQuery {
        start: utc(start),
        end: utc(end),
        timezone: "UTC".into(),
        limit: 100,
    }
}
fn recurring_event(
    engine: &mut Engine,
    schedule: EventSchedule,
    frequency: Frequency,
    count: u32,
) -> ultratasks_core::Event {
    let Outcome::Event(event) = engine
        .execute(Command::CreateEvent {
            title: "series".into(),
            schedule,
        })
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Event(event) = engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: event.revision,
            recurrence: Some(Recurrence {
                frequency,
                interval: 1,
                count: Some(count),
                until: None,
            }),
        })
        .unwrap()
    else {
        panic!()
    };
    event
}

#[test]
fn same_entity_twice_in_batch_is_one_undoable_transition() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Task(task) = engine.execute(create_task("original")).unwrap() else {
        panic!()
    };
    engine
        .execute_batch(vec![
            Command::UpdateTask {
                id: task.id,
                expected_revision: task.revision,
                title: "renamed".into(),
                completed: false,
                due: None,
                list_id: None,
            },
            Command::SetTaskCompleted {
                id: task.id,
                expected_revision: task.revision + 1,
                completed: true,
            },
        ])
        .unwrap();
    let updated = engine.task(task.id).unwrap();
    assert_eq!(updated.title, "renamed");
    assert!(updated.completed);
    engine.undo().unwrap();
    let undone = engine.task(task.id).unwrap();
    assert_eq!(undone.title, "original");
    assert!(!undone.completed);
    engine.redo().unwrap();
    let redone = engine.task(task.id).unwrap();
    assert_eq!(redone.title, "renamed");
    assert!(redone.completed);
}

#[test]
fn failed_batch_does_not_leave_a_partial_edit_or_history_entry() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Task(task) = engine.execute(create_task("original")).unwrap() else {
        panic!()
    };
    assert!(
        engine
            .execute_batch(vec![
                Command::SetTaskCompleted {
                    id: task.id,
                    expected_revision: task.revision,
                    completed: true
                },
                Command::UpdateTask {
                    id: task.id,
                    expected_revision: task.revision + 1,
                    title: "\n".into(),
                    completed: true,
                    due: None,
                    list_id: None
                },
            ])
            .is_err()
    );
    assert_eq!(engine.task(task.id).unwrap(), task);
    engine.undo().unwrap();
    assert!(
        engine.task(task.id).is_err(),
        "the only committed history entry was the initial creation"
    );
}

#[test]
fn idempotent_retry_after_restart_returns_original_result_not_latest_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("idempotency.db");
    let mut engine = Engine::open_for_actor(&path, "mcp").unwrap();
    let commands = vec![create_task("original")];
    let original = engine
        .execute_idempotent("request-1", commands.clone())
        .unwrap();
    let Outcome::Task(task) = &original[0] else {
        panic!()
    };
    engine
        .execute(Command::SetTaskCompleted {
            id: task.id,
            expected_revision: task.revision,
            completed: true,
        })
        .unwrap();
    drop(engine);
    let mut engine = Engine::open_for_actor(&path, "mcp").unwrap();
    let retry = engine.execute_idempotent("request-1", commands).unwrap();
    assert_eq!(
        serde_json::to_value(&retry).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
    assert!(engine.task(task.id).unwrap().completed);
    assert!(
        engine
            .execute_idempotent("request-1", vec![create_task("different intent")])
            .is_err()
    );
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
    engine.undo().unwrap();
    assert!(
        !engine.task(task.id).unwrap().completed,
        "retry must not append history"
    );
}

#[test]
fn failed_idempotent_batch_does_not_consume_its_request_id() {
    let mut engine = Engine::in_memory().unwrap();
    assert!(
        engine
            .execute_idempotent(
                "retry-after-validation-error",
                vec![create_task("valid"), create_task("")]
            )
            .is_err()
    );
    assert!(engine.tasks(&TaskQuery::default()).unwrap().is_empty());
    assert!(!engine.history_status().unwrap().can_undo);
    engine
        .execute_idempotent(
            "retry-after-validation-error",
            vec![create_task("corrected")],
        )
        .unwrap();
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
}

#[test]
fn recurrence_skips_nonexistent_dst_wall_time_without_consuming_count() {
    let mut engine = Engine::in_memory().unwrap();
    recurring_event(
        &mut engine,
        EventSchedule::Timed {
            start: utc("2026-03-07T07:30:00Z"),
            end: utc("2026-03-07T08:00:00Z"),
            timezone: "America/New_York".into(),
        },
        Frequency::Daily,
        2,
    );
    let events = engine
        .event_occurrences(&query("2026-03-07T00:00:00Z", "2026-03-11T00:00:00Z"), None)
        .unwrap();
    assert_eq!(
        events.iter().map(|e| e.original_date).collect::<Vec<_>>(),
        vec![date("2026-03-07"), date("2026-03-09")]
    );
    assert!(
        matches!(&events[1].schedule, EventSchedule::Timed { start, end, .. } if *start == utc("2026-03-09T06:30:00Z") && *end == utc("2026-03-09T07:00:00Z"))
    );
}

#[test]
fn recurrence_fold_chooses_earliest_instant_and_preserves_elapsed_duration() {
    let mut engine = Engine::in_memory().unwrap();
    recurring_event(
        &mut engine,
        EventSchedule::Timed {
            start: utc("2026-10-31T05:30:00Z"),
            end: utc("2026-10-31T06:30:00Z"),
            timezone: "America/New_York".into(),
        },
        Frequency::Daily,
        2,
    );
    let events = engine
        .event_occurrences(&query("2026-11-01T00:00:00Z", "2026-11-02T00:00:00Z"), None)
        .unwrap();
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0].schedule, EventSchedule::Timed { start, end, .. } if *start == utc("2026-11-01T05:30:00Z") && *end == utc("2026-11-01T06:30:00Z"))
    );
}

#[test]
fn monthly_31st_skips_short_months_instead_of_clamping() {
    let mut engine = Engine::in_memory().unwrap();
    recurring_event(
        &mut engine,
        EventSchedule::AllDay {
            start: date("2026-01-31"),
            end: date("2026-02-01"),
        },
        Frequency::Monthly,
        4,
    );
    let events = engine
        .event_occurrences(&query("2026-01-01T00:00:00Z", "2026-08-01T00:00:00Z"), None)
        .unwrap();
    assert_eq!(
        events.iter().map(|e| e.original_date).collect::<Vec<_>>(),
        vec![
            date("2026-01-31"),
            date("2026-03-31"),
            date("2026-05-31"),
            date("2026-07-31")
        ]
    );
}

#[test]
fn moved_occurrence_is_found_by_effective_time_but_keeps_original_identity() {
    let mut engine = Engine::in_memory().unwrap();
    let event = recurring_event(
        &mut engine,
        EventSchedule::AllDay {
            start: date("2026-01-01"),
            end: date("2026-01-02"),
        },
        Frequency::Daily,
        2,
    );
    engine
        .execute(Command::SetEventException {
            id: event.id,
            expected_revision: event.revision,
            date: date("2026-01-01"),
            exception: Some(EventException::Override {
                title: Some("moved".into()),
                schedule: EventSchedule::AllDay {
                    start: date("2026-06-01"),
                    end: date("2026-06-02"),
                },
            }),
        })
        .unwrap();
    let events = engine
        .event_occurrences(&query("2026-06-01T00:00:00Z", "2026-06-02T00:00:00Z"), None)
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].original_date, date("2026-01-01"));
    assert_eq!(events[0].title, "moved");
    assert_eq!(
        engine
            .event_occurrences(&query("2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"), None)
            .unwrap()
            .len(),
        0
    );
    engine.undo().unwrap();
    assert_eq!(
        engine
            .event_occurrences(&query("2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"), None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn completing_one_recurring_task_occurrence_does_not_complete_its_neighbors() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Task(task) = engine
        .execute(Command::CreateTask {
            title: "daily".into(),
            due: Some(DueDate::Date(date("2026-01-01"))),
            list_id: None,
        })
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Task(task) = engine
        .execute(Command::SetTaskRecurrence {
            id: task.id,
            expected_revision: task.revision,
            recurrence: Some(Recurrence {
                frequency: Frequency::Daily,
                interval: 1,
                count: Some(3),
                until: None,
            }),
        })
        .unwrap()
    else {
        panic!()
    };
    engine
        .execute(Command::SetTaskException {
            id: task.id,
            expected_revision: task.revision,
            date: date("2026-01-02"),
            exception: Some(TaskException::Override {
                title: None,
                due: DueDate::Date(date("2026-01-02")),
                completed: true,
            }),
        })
        .unwrap();
    let occurrences = engine
        .task_occurrences(&query("2026-01-01T00:00:00Z", "2026-01-04T00:00:00Z"))
        .unwrap();
    assert_eq!(
        occurrences.iter().map(|v| v.completed).collect::<Vec<_>>(),
        vec![false, true, false]
    );
    assert!(!engine.task(task.id).unwrap().completed);
}

#[test]
fn deleting_task_cascades_reminders_on_both_task_and_associated_blocks_atomically() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Task(task) = engine.execute(create_task("parent")).unwrap() else {
        panic!()
    };
    let Outcome::TimeBlock(block) = engine
        .execute(Command::ScheduleTask {
            task_id: task.id,
            expected_task_revision: task.revision,
            start: utc("2026-11-09T09:00:00Z"),
            end: utc("2026-11-09T10:00:00Z"),
            timezone: "UTC".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    for target in [EntityId::Task(task.id), EntityId::TimeBlock(block.id)] {
        engine
            .execute(Command::CreateReminder {
                target,
                trigger: ReminderTrigger::At(utc("2026-11-09T08:00:00Z")),
            })
            .unwrap();
    }
    engine
        .execute(Command::DeleteTask {
            id: task.id,
            expected_revision: task.revision,
        })
        .unwrap();
    assert!(engine.task(task.id).is_err());
    assert!(engine.time_block(block.id).is_err());
    engine.undo().unwrap();
    assert_eq!(engine.task(task.id).unwrap().title, "parent");
    assert_eq!(engine.time_block(block.id).unwrap().task_id, task.id);
}

#[test]
fn json_import_rejects_duplicate_ids_and_orphaned_children_without_partial_writes() {
    let mut source = Engine::in_memory().unwrap();
    let Outcome::Task(task) = source.execute(create_task("parent")).unwrap() else {
        panic!()
    };
    source
        .execute(Command::ScheduleTask {
            task_id: task.id,
            expected_task_revision: task.revision,
            start: utc("2026-11-09T09:00:00Z"),
            end: utc("2026-11-09T10:00:00Z"),
            timezone: "UTC".into(),
        })
        .unwrap();
    let exported = source.export_json().unwrap();
    let mut duplicate: serde_json::Value = serde_json::from_str(&exported).unwrap();
    let records = duplicate["records"].as_array_mut().unwrap();
    records.push(records[0].clone());
    let mut target = Engine::in_memory().unwrap();
    assert!(
        target
            .import_json(&duplicate.to_string(), ImportMode::Empty)
            .is_err()
    );
    assert!(target.tasks(&TaskQuery::default()).unwrap().is_empty());
    let mut orphan: serde_json::Value = serde_json::from_str(&exported).unwrap();
    orphan["records"]
        .as_array_mut()
        .unwrap()
        .retain(|record| record["entity"].get("Task").is_none());
    assert!(
        target
            .import_json(&orphan.to_string(), ImportMode::Empty)
            .is_err()
    );
    assert!(
        target
            .time_blocks(&query("2026-11-09T00:00:00Z", "2026-11-10T00:00:00Z"))
            .unwrap()
            .is_empty()
    );
    assert!(!target.history_status().unwrap().can_undo);
    target.import_json(&exported, ImportMode::Empty).unwrap();
    assert_eq!(target.task(task.id).unwrap(), task);
}

#[test]
fn json_import_cannot_attach_live_child_to_parent_tombstone() {
    let mut source = Engine::in_memory().unwrap();
    let Outcome::Task(task) = source.execute(create_task("parent")).unwrap() else {
        panic!()
    };
    source
        .execute(Command::ScheduleTask {
            task_id: task.id,
            expected_task_revision: task.revision,
            start: utc("2026-11-09T09:00:00Z"),
            end: utc("2026-11-09T10:00:00Z"),
            timezone: "UTC".into(),
        })
        .unwrap();
    let mut document: serde_json::Value =
        serde_json::from_str(&source.export_json().unwrap()).unwrap();
    for record in document["records"].as_array_mut().unwrap() {
        if record["entity"].get("Task").is_some() {
            record["deleted"] = true.into();
        }
    }
    let mut target = Engine::in_memory().unwrap();
    assert!(
        target
            .import_json(&document.to_string(), ImportMode::Empty)
            .is_err()
    );
    assert!(target.tasks(&TaskQuery::default()).unwrap().is_empty());
}

#[test]
fn merge_collision_rolls_back_preceding_new_records() {
    let mut source = Engine::in_memory().unwrap();
    let Outcome::Task(existing) = source.execute(create_task("old")).unwrap() else {
        panic!()
    };
    let mut target = Engine::in_memory().unwrap();
    target
        .import_json(&source.export_json().unwrap(), ImportMode::Empty)
        .unwrap();
    target
        .execute(Command::SetTaskCompleted {
            id: existing.id,
            expected_revision: existing.revision,
            completed: true,
        })
        .unwrap();
    let Outcome::Task(new_task) = source
        .execute(create_task("new record before collision"))
        .unwrap()
    else {
        panic!()
    };
    let mut document: serde_json::Value =
        serde_json::from_str(&source.export_json().unwrap()).unwrap();
    document["records"].as_array_mut().unwrap().reverse();
    assert!(
        target
            .import_json(&document.to_string(), ImportMode::Merge)
            .is_err()
    );
    assert!(target.task(new_task.id).is_err());
    assert!(target.task(existing.id).unwrap().completed);
    assert_eq!(target.tasks(&TaskQuery::default()).unwrap().len(), 1);
}

#[test]
fn backups_and_restores_never_replace_an_existing_destination() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    let foreign_path = dir.path().join("important.db");
    let backup_path = dir.path().join("backup.db");
    let restored_path = dir.path().join("restored.db");
    let mut source = Engine::open(&path).unwrap();
    let Outcome::Task(task) = source
        .execute(create_task("persisted in active WAL"))
        .unwrap()
    else {
        panic!()
    };
    let foreign = rusqlite::Connection::open(&foreign_path).unwrap();
    foreign
        .execute_batch("CREATE TABLE notes(body TEXT); INSERT INTO notes VALUES('irreplaceable');")
        .unwrap();
    assert!(source.backup_to(&foreign_path).is_err());
    source.backup_to(&backup_path).unwrap();
    assert!(Engine::restore_backup(&backup_path, &foreign_path).is_err());
    assert_eq!(
        foreign
            .query_row("SELECT body FROM notes", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "irreplaceable"
    );
    Engine::restore_backup(&backup_path, &restored_path).unwrap();
    let restored = Engine::open(&restored_path).unwrap();
    assert_eq!(restored.task(task.id).unwrap(), task);
    assert!(Engine::restore_backup(&foreign_path, dir.path().join("invalid-copy.db")).is_err());
    assert!(!dir.path().join("invalid-copy.db").exists());
}

#[test]
fn restore_does_not_accept_divergence_between_event_payload_and_query_columns() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("inconsistent.db");
    let restored_path = dir.path().join("restored.db");
    let mut source = Engine::open(&source_path).unwrap();
    let Outcome::Event(event) = source
        .execute(Command::CreateEvent {
            title: "must remain visible".into(),
            schedule: EventSchedule::Timed {
                start: utc("2026-11-09T09:00:00Z"),
                end: utc("2026-11-09T10:00:00Z"),
                timezone: "UTC".into(),
            },
        })
        .unwrap()
    else {
        panic!()
    };
    drop(source);
    let connection = rusqlite::Connection::open(&source_path).unwrap();
    connection.execute("UPDATE events SET start_utc='2027-01-01T09:00:00.000000000Z', end_utc='2027-01-01T10:00:00.000000000Z' WHERE id=?1", [event.id.to_string()]).unwrap();
    drop(connection);
    match Engine::restore_backup(&source_path, &restored_path) {
        Err(_) => assert!(!restored_path.exists()),
        Ok(()) => {
            let restored = Engine::open(&restored_path).unwrap();
            let events = restored
                .events(&query("2026-11-09T00:00:00Z", "2026-11-10T00:00:00Z"))
                .unwrap();
            assert_eq!(
                events,
                vec![event],
                "a restore must reject or repair inconsistent query columns"
            );
        }
    }
}

fn synchronize(client: &mut Engine, server: &mut Engine) {
    use ultratasks_core::sync::SyncPushResult;
    let generation = server.sync_server_snapshot().unwrap().generation;
    assert!(client.sync_attach(generation).unwrap());
    while let Some(request) = client.sync_prepare_push().unwrap() {
        let response = server.sync_server_push(&request).unwrap();
        assert!(
            matches!(response, SyncPushResult::Accepted { .. }),
            "expected a nonconflicting push"
        );
        client.sync_accept_push_result(&request, &response).unwrap();
    }
    loop {
        let position = client.sync_position().unwrap();
        let response = server
            .sync_server_pull(generation, position.cursor, 100)
            .unwrap();
        client.sync_apply_pull(&response).unwrap();
        if !response.has_more {
            break;
        }
    }
}

#[test]
fn sync_queue_failure_rolls_back_data_history_and_idempotency_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("queue-failure.db");
    let mut engine = Engine::open(&path).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_sync BEFORE INSERT ON sync_outbox BEGIN SELECT RAISE(ABORT,'injected queue failure'); END;").unwrap();
    assert!(
        engine
            .execute_idempotent("atomic-queue", vec![create_task("must roll back")])
            .is_err()
    );
    assert!(engine.tasks(&TaskQuery::default()).unwrap().is_empty());
    assert!(!engine.history_status().unwrap().can_undo);
    assert_eq!(engine.sync_position().unwrap().pending_operations, 0);
    connection.execute_batch("DROP TRIGGER fail_sync;").unwrap();
    engine
        .execute_idempotent("atomic-queue", vec![create_task("correctly retried")])
        .unwrap();
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
    assert_eq!(engine.sync_position().unwrap().pending_operations, 1);
}

#[test]
fn sync_first_push_accepts_related_records_regardless_of_wire_order() {
    let mut source = Engine::in_memory().unwrap();
    let mut target = Engine::in_memory().unwrap();
    let mut server = Engine::in_memory().unwrap();
    server.sync_server_start().unwrap();
    let Outcome::List(list) = source
        .execute(Command::CreateList {
            title: "list".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Task(task) = source
        .execute(Command::CreateTask {
            title: "task".into(),
            due: None,
            list_id: Some(list.id),
        })
        .unwrap()
    else {
        panic!()
    };
    let Outcome::TimeBlock(block) = source
        .execute(Command::ScheduleTask {
            task_id: task.id,
            expected_task_revision: task.revision,
            start: utc("2026-11-09T09:00:00Z"),
            end: utc("2026-11-09T10:00:00Z"),
            timezone: "UTC".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    source
        .execute(Command::CreateReminder {
            target: EntityId::TimeBlock(block.id),
            trigger: ReminderTrigger::Before { minutes: 15 },
        })
        .unwrap();
    synchronize(&mut source, &mut server);
    synchronize(&mut target, &mut server);
    assert_eq!(target.task(task.id).unwrap().list_id, Some(list.id));
    assert_eq!(target.time_block(block.id).unwrap().task_id, task.id);
    assert_eq!(target.reminders(10).unwrap().len(), 1);
}

#[test]
fn foreign_same_value_sync_edit_blocks_old_local_undo() {
    let mut alice = Engine::in_memory().unwrap();
    let mut bob = Engine::in_memory().unwrap();
    let mut server = Engine::in_memory().unwrap();
    server.sync_server_start().unwrap();
    let Outcome::Task(task) = alice.execute(create_task("same value")).unwrap() else {
        panic!()
    };
    synchronize(&mut alice, &mut server);
    synchronize(&mut bob, &mut server);
    let bob_task = bob.task(task.id).unwrap();
    bob.execute(Command::UpdateTask {
        id: task.id,
        expected_revision: bob_task.revision,
        title: bob_task.title,
        completed: bob_task.completed,
        due: bob_task.due,
        list_id: bob_task.list_id,
    })
    .unwrap();
    synchronize(&mut bob, &mut server);
    synchronize(&mut alice, &mut server);
    assert!(
        alice.undo().is_err(),
        "a foreign revision must invalidate Undo even if field values did not change"
    );
    assert_eq!(alice.task(task.id).unwrap().title, "same value");
}

#[test]
fn sync_server_deduplicates_lost_ack_across_restart_and_rejects_id_collision() {
    let mut client = Engine::in_memory().unwrap();
    let mut server = Engine::in_memory().unwrap();
    let generation = server.sync_server_start().unwrap();
    client.sync_attach(generation).unwrap();
    client.execute(create_task("once")).unwrap();
    let request = client.sync_prepare_push().unwrap().unwrap();
    let original = server.sync_server_push(&request).unwrap();
    server.sync_server_start().unwrap();
    let retry = server.sync_server_push(&request).unwrap();
    assert_eq!(
        serde_json::to_value(&retry).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(server.sync_server_snapshot().unwrap().cursor, 1);
    assert_eq!(server.tasks(&TaskQuery::default()).unwrap().len(), 1);
    let mut collision = request.clone();
    if let ultratasks_core::Entity::Task(task) = &mut collision.mutations[0].record.entity {
        task.title = "different".into();
    }
    assert!(server.sync_server_push(&collision).is_err());
    assert_eq!(
        server.tasks(&TaskQuery::default()).unwrap()[0].title,
        "once"
    );
    client.sync_accept_push_result(&request, &retry).unwrap();
    assert_eq!(client.sync_position().unwrap().pending_operations, 0);
}

#[test]
fn offline_edits_to_distinct_tasks_in_one_list_do_not_conflict_on_unchanged_parent() {
    let mut alice = Engine::in_memory().unwrap();
    let mut bob = Engine::in_memory().unwrap();
    let mut server = Engine::in_memory().unwrap();
    server.sync_server_start().unwrap();
    let Outcome::List(list) = alice
        .execute(Command::CreateList {
            title: "shared list".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    let mut tasks = Vec::new();
    for title in ["Alice edits this", "Bob edits this"] {
        let Outcome::Task(task) = alice
            .execute(Command::CreateTask {
                title: title.into(),
                due: None,
                list_id: Some(list.id),
            })
            .unwrap()
        else {
            panic!()
        };
        tasks.push(task.id);
    }
    synchronize(&mut alice, &mut server);
    synchronize(&mut bob, &mut server);
    let first = alice.task(tasks[0]).unwrap();
    let second = bob.task(tasks[1]).unwrap();
    alice
        .execute(Command::SetTaskCompleted {
            id: first.id,
            expected_revision: first.revision,
            completed: true,
        })
        .unwrap();
    bob.execute(Command::SetTaskCompleted {
        id: second.id,
        expected_revision: second.revision,
        completed: true,
    })
    .unwrap();
    synchronize(&mut alice, &mut server);
    synchronize(&mut bob, &mut server);
    synchronize(&mut alice, &mut server);
    for id in tasks {
        assert!(alice.task(id).unwrap().completed);
        assert!(bob.task(id).unwrap().completed);
        assert!(server.task(id).unwrap().completed);
    }
}

#[test]
fn ics_exception_in_second_dst_fold_cannot_modify_the_first_fold_occurrence() {
    let document = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//UltraTasks Test//EN\r\nBEGIN:VEVENT\r\nUID:fold-case@example.invalid\r\nSUMMARY:Fold identity\r\nDTSTART;TZID=America/New_York:20261031T013000\r\nDTEND;TZID=America/New_York:20261031T023000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE:20261101T063000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut engine = Engine::in_memory().unwrap();
    assert!(
        engine.import_ics(document, ImportMode::Empty).is_err(),
        "06:30Z identifies the second 01:30; the recurring event is at the first 01:30 (05:30Z)"
    );
    assert!(
        engine
            .event_occurrences(&query("2026-10-31T00:00:00Z", "2026-11-03T00:00:00Z"), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reset_approval_cannot_discard_local_edits_added_while_fetching_remote_snapshot() {
    use ultratasks_core::sync::SyncResolution;
    let mut local = Engine::in_memory().unwrap();
    let mut server = Engine::in_memory().unwrap();
    server.sync_server_start().unwrap();
    synchronize(&mut local, &mut server);
    let approval_guard = local.sync_reset_guard().unwrap();
    server.sync_server_start().unwrap();
    let remote = server.sync_server_snapshot().unwrap();
    let Outcome::Task(new_task) = local
        .execute(create_task("created after reset was approved"))
        .unwrap()
    else {
        panic!()
    };
    let before = local.sync_position().unwrap();
    assert!(
        local
            .sync_resolve_reset_guarded(&remote, SyncResolution::KeepRemote, &approval_guard)
            .is_err()
    );
    assert_eq!(local.task(new_task.id).unwrap(), new_task);
    let after = local.sync_position().unwrap();
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.cursor, before.cursor);
    assert_eq!(after.pending_operations, before.pending_operations);
}

#[test]
fn resolving_one_sync_conflict_preserves_undo_for_unchanged_unrelated_records() {
    use ultratasks_core::sync::{SyncPushResult, SyncResolution};
    let directory = tempfile::tempdir().unwrap();
    let local_path = directory.path().join("local.db");
    let mut unrelated_owner = Engine::open_for_actor(&local_path, "unrelated-owner").unwrap();
    let Outcome::Task(unrelated) = unrelated_owner
        .execute(create_task("untouched task"))
        .unwrap()
    else {
        panic!()
    };
    let mut alice = Engine::open_for_actor(&local_path, "conflict-owner").unwrap();
    let mut bob = Engine::in_memory().unwrap();
    let mut server = Engine::in_memory().unwrap();
    server.sync_server_start().unwrap();
    let Outcome::Task(shared) = alice.execute(create_task("conflicting task")).unwrap() else {
        panic!()
    };
    synchronize(&mut alice, &mut server);
    synchronize(&mut bob, &mut server);
    let alice_task = alice.task(shared.id).unwrap();
    let bob_task = bob.task(shared.id).unwrap();
    for (engine, task, title) in [
        (&mut alice, alice_task, "Alice"),
        (&mut bob, bob_task, "Bob"),
    ] {
        engine
            .execute(Command::UpdateTask {
                id: task.id,
                expected_revision: task.revision,
                title: title.into(),
                completed: false,
                due: None,
                list_id: None,
            })
            .unwrap();
    }
    synchronize(&mut bob, &mut server);
    let request = alice.sync_prepare_push().unwrap().unwrap();
    let response = server.sync_server_push(&request).unwrap();
    assert!(matches!(response, SyncPushResult::Conflict { .. }));
    alice.sync_accept_push_result(&request, &response).unwrap();
    alice
        .sync_resolve_conflict(SyncResolution::KeepRemote)
        .unwrap();
    assert_eq!(alice.task(shared.id).unwrap().title, "Bob");
    unrelated_owner.undo().expect(
        "resolving a conflict must not fabricate a foreign edit on an unrelated known record",
    );
    assert!(unrelated_owner.task(unrelated.id).is_err());
}
