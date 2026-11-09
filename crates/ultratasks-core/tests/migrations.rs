use rusqlite::{Connection, params};
use ultratasks_core::{Command, Engine, EventId, Outcome, TaskId};

#[test]
fn actual_v2_schema_preserves_legacy_records_and_undo_before_new_commands() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    let task_id = TaskId::new();
    let event_id = EventId::new();
    let legacy = Connection::open(&path).unwrap();
    legacy
        .execute_batch(include_str!("fixtures/schema-v2.sql"))
        .unwrap();
    legacy
        .execute(
            "INSERT INTO tasks VALUES (?1,'移行前',0,NULL,NULL,1,0)",
            [task_id.to_string()],
        )
        .unwrap();
    let after = serde_json::json!([{
        "entity":{"Task":{"id":task_id,"title":"移行前","completed":false,"due":null,"list_id":null,"revision":1}},
        "deleted":false
    }]);
    let mut before = after.clone();
    before[0]["deleted"] = true.into();
    before[0]["entity"]["Task"]["revision"] = 0.into();
    legacy
        .execute(
            "INSERT INTO operations(actor,before_json,after_json) VALUES('local',?1,?2)",
            params![before.to_string(), after.to_string()],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO history_guards VALUES('local','task',?1,1)",
            [task_id.to_string()],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO events VALUES(?1,'予定',?2,NULL,NULL,'2026-11-09','2026-11-10',1,0)",
            params![
                event_id.to_string(),
                r#"{"AllDay":{"start":"2026-11-09","end":"2026-11-10"}}"#
            ],
        )
        .unwrap();
    drop(legacy);

    let mut engine = Engine::open(&path).unwrap();
    let task = engine.task(task_id).unwrap();
    assert_eq!(task.title, "移行前");
    assert!(task.recurrence.is_none());
    assert!(task.exceptions.is_empty());
    assert!(engine.event(event_id).unwrap().calendar_id.is_none());
    engine.undo().unwrap();
    assert!(engine.task(task_id).is_err());
    engine.redo().unwrap();
    assert_eq!(engine.task(task_id).unwrap().revision, 3);
    let Outcome::Calendar(calendar) = engine
        .execute(Command::CreateCalendar {
            title: "仕事".into(),
        })
        .unwrap()
    else {
        panic!()
    };
    engine
        .execute(Command::AssignEventCalendar {
            id: event_id,
            expected_revision: 1,
            calendar_id: Some(calendar.id),
        })
        .unwrap();
    drop(engine);
    let engine = Engine::open(path).unwrap();
    assert_eq!(
        engine.event(event_id).unwrap().calendar_id,
        Some(calendar.id)
    );
    assert_eq!(engine.task(task_id).unwrap().title, "移行前");
}
