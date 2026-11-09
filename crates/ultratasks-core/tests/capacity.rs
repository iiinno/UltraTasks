//! Boundary fixtures deliberately inject a near-full accounting projection while
//! the engine is open. This exercises transaction capacity enforcement without
//! allocating 100,000 real entities; it is not a valid database recovery fixture.
use rusqlite::{Connection, params};
use ultratasks_core::*;
const MAX_BYTES: i64 = 32 * 1024 * 1024 - 1024;

fn create(engine: &mut Engine, title: &str) -> Task {
    let Outcome::Task(value) = engine
        .execute(Command::CreateTask {
            title: title.into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    else {
        panic!()
    };
    value
}
fn change(task: &Task, title: &str) -> Command {
    Command::UpdateTask {
        id: task.id,
        expected_revision: task.revision,
        title: title.into(),
        completed: task.completed,
        due: task.due.clone(),
        list_id: task.list_id,
    }
}
fn counts(connection: &Connection) -> (i64, i64, i64, i64) {
    connection.query_row("SELECT (SELECT count(*) FROM tasks),(SELECT count(*) FROM operations),(SELECT count(*) FROM sync_outbox),(SELECT count(*) FROM requests)",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap()
}
fn fill_bytes(connection: &Connection, ballast: TaskId, total: i64) {
    let previous: i64 = connection
        .query_row(
            "SELECT bytes FROM sync_capacity_total WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE sync_capacity_records SET bytes=bytes+?1 WHERE kind='task' AND id=?2",
            params![total - previous, ballast.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE sync_capacity_total SET bytes=?1 WHERE singleton=1",
            [total],
        )
        .unwrap();
}

#[test]
fn rejected_dataset_byte_growth_rolls_back_record_history_outbox_and_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capacity.sqlite3");
    let mut engine = Engine::open(&path).unwrap();
    let ballast = create(&mut engine, "ballast");
    let db = Connection::open(&path).unwrap();
    fill_bytes(&db, ballast.id, MAX_BYTES - 1);
    let before = counts(&db);
    let result = engine.execute_idempotent(
        "retry-after-space",
        vec![Command::CreateTask {
            title: "must roll back".into(),
            due: None,
            list_id: None,
        }],
    );
    assert!(matches!(result,Err(Error::InvalidInput(message)) if message.contains("capacity")));
    assert_eq!(counts(&db), before);
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
    assert_eq!(
        db.query_row(
            "SELECT bytes FROM sync_capacity_total WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        MAX_BYTES - 1
    );
    // No idempotency receipt was consumed: after space becomes available, retry works.
    let actual = serde_json::to_vec(&engine.snapshots().unwrap()[0])
        .unwrap()
        .len() as i64
        + 1;
    db.execute(
        "UPDATE sync_capacity_records SET bytes=?1 WHERE kind='task' AND id=?2",
        params![actual, ballast.id.to_string()],
    )
    .unwrap();
    db.execute(
        "UPDATE sync_capacity_total SET bytes=?1 WHERE singleton=1",
        [actual],
    )
    .unwrap();
    engine
        .execute_idempotent(
            "retry-after-space",
            vec![Command::CreateTask {
                title: "must roll back".into(),
                due: None,
                list_id: None,
            }],
        )
        .unwrap();
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 2);
}

#[test]
fn capacity_checks_final_transaction_size_not_intermediate_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capacity.sqlite3");
    let mut engine = Engine::open(&path).unwrap();
    let ballast = create(&mut engine, "ballast");
    let grow = create(&mut engine, "a");
    let shrink = create(&mut engine, &"b".repeat(300));
    let db = Connection::open(&path).unwrap();
    fill_bytes(&db, ballast.id, MAX_BYTES - 10);
    engine
        .execute_batch(vec![change(&grow, &"a".repeat(200)), change(&shrink, "b")])
        .unwrap();
    assert_eq!(engine.task(grow.id).unwrap().title.len(), 200);
    assert_eq!(engine.task(shrink.id).unwrap().title, "b");
    let bytes: i64 = db
        .query_row(
            "SELECT bytes FROM sync_capacity_total WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(bytes <= MAX_BYTES - 100);
    engine.undo().unwrap();
    assert_eq!(engine.task(grow.id).unwrap().title, "a");
    assert_eq!(engine.task(shrink.id).unwrap().title.len(), 300);
}

#[test]
fn deleted_entities_continue_to_count_toward_record_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capacity.sqlite3");
    let mut engine = Engine::open(&path).unwrap();
    let task = create(&mut engine, "ballast");
    engine
        .execute(Command::DeleteTask {
            id: task.id,
            expected_revision: 1,
        })
        .unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT records FROM sync_capacity_total WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    db.execute(
        "UPDATE sync_capacity_total SET records=100000 WHERE singleton=1",
        [],
    )
    .unwrap();
    let before = counts(&db);
    assert!(
        engine
            .execute(Command::CreateList {
                title: "one too many".into()
            })
            .is_err()
    );
    assert!(engine.lists(100).unwrap().is_empty());
    assert_eq!(counts(&db), before);
    assert_eq!(
        db.query_row(
            "SELECT records FROM sync_capacity_total WHERE singleton=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        100000
    );
}
