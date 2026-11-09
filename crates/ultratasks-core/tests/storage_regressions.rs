use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;
use ultratasks_core::{CalendarQuery, Command, DueDate, Engine, Error, EventSchedule, Outcome};

fn utc(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

#[test]
fn opening_a_foreign_database_does_not_modify_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE notes(body TEXT); INSERT INTO notes VALUES('do not touch'); PRAGMA journal_mode=DELETE;").unwrap();
    assert!(matches!(
        Engine::open(&path),
        Err(Error::UnrecognizedDatabase)
    ));
    assert_eq!(
        db.query_row("SELECT body FROM notes", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "do not touch"
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn newer_owned_schema_is_refused_before_journal_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.db");
    drop(Engine::open(&path).unwrap());
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA user_version=999;")
        .unwrap();
    assert!(matches!(
        Engine::open(&path),
        Err(Error::UnsupportedSchema(999))
    ));
    assert_eq!(
        db.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
}

#[test]
fn missing_table_in_marked_database_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.db");
    drop(Engine::open(&path).unwrap());
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE events;").unwrap();
    assert!(matches!(
        Engine::open(&path),
        Err(Error::UnrecognizedDatabase)
    ));
}

#[test]
fn migrating_v1_adds_default_page_index_without_changing_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migration.db");
    let mut engine = Engine::open(&path).unwrap();
    let Outcome::Task(task) = engine
        .execute(Command::CreateTask {
            title: "移行前のタスク".into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    else {
        panic!()
    };
    drop(engine);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP INDEX tasks_order; PRAGMA user_version=1;")
        .unwrap();
    let mut engine = Engine::open(&path).unwrap();
    assert_eq!(engine.task(task.id).unwrap(), task);
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='tasks_order'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    engine.undo().unwrap();
    assert!(engine.task(task.id).is_err());
}

#[test]
fn failure_in_middle_of_cascade_rolls_back_data_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atomic.db");
    let mut engine = Engine::open(&path).unwrap();
    let Outcome::Task(task) = engine
        .execute(Command::CreateTask {
            title: "保持する".into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    else {
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
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_block_delete BEFORE UPDATE ON time_blocks WHEN NEW.deleted=1 BEGIN SELECT RAISE(ABORT,'injected storage failure'); END;").unwrap();
    assert!(matches!(
        engine.execute(Command::DeleteTask {
            id: task.id,
            expected_revision: task.revision
        }),
        Err(Error::Database(_))
    ));
    assert_eq!(engine.task(task.id).unwrap(), task);
    assert_eq!(engine.time_block(block.id).unwrap(), block);
    assert_eq!(
        db.query_row("SELECT count(*) FROM operations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    db.execute_batch("DROP TRIGGER fail_block_delete;").unwrap();
    engine.undo().unwrap();
    assert!(engine.time_block(block.id).is_err());
    assert_eq!(engine.task(task.id).unwrap(), task);
}

#[test]
fn unsupported_leap_seconds_are_rejected_without_writing() {
    let mut engine = Engine::in_memory().unwrap();
    let leap = utc("2016-12-31T23:59:60Z");
    assert!(
        engine
            .execute(Command::CreateEvent {
                title: "leap".into(),
                schedule: EventSchedule::Timed {
                    start: leap,
                    end: utc("2017-01-01T00:01:00Z"),
                    timezone: "UTC".into()
                }
            })
            .is_err()
    );
    assert!(
        engine
            .execute(Command::CreateTask {
                title: "leap due".into(),
                due: Some(DueDate::At {
                    at: leap,
                    timezone: "UTC".into()
                }),
                list_id: None
            })
            .is_err()
    );
    assert!(!engine.history_status().unwrap().can_undo);
}

#[test]
fn local_date_boundaries_and_rounded_query_end_are_validated() {
    let engine = Engine::in_memory().unwrap();
    assert!(
        engine
            .events(&CalendarQuery {
                start: Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0).unwrap(),
                end: Utc.with_ymd_and_hms(1, 1, 2, 0, 0, 0).unwrap(),
                timezone: "America/New_York".into(),
                limit: 10
            })
            .is_err()
    );
    assert!(
        engine
            .events(&CalendarQuery {
                start: utc("9999-12-31T00:00:00Z"),
                end: utc("9999-12-31T12:00:00Z"),
                timezone: "UTC".into(),
                limit: 10
            })
            .is_err()
    );
}

#[test]
fn history_pruning_is_bounded_and_preserves_the_retained_boundary() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Task(task) = engine
        .execute(Command::CreateTask {
            title: "retained task".into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    else {
        panic!()
    };
    for _ in 0..1002 {
        let current = engine.task(task.id).unwrap();
        engine
            .execute(Command::SetTaskCompleted {
                id: task.id,
                expected_revision: current.revision,
                completed: !current.completed,
            })
            .unwrap();
    }
    for _ in 0..1000 {
        engine.undo().unwrap();
    }
    assert!(matches!(engine.undo(), Err(Error::NoHistory)));
    assert!(!engine.task(task.id).unwrap().completed);
    for _ in 0..1000 {
        engine.redo().unwrap();
    }
    assert!(matches!(engine.redo(), Err(Error::NoHistory)));
    assert!(!engine.task(task.id).unwrap().completed);
}
