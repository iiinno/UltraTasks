use crate::{
    EntityId, Event, EventSchedule, ListId, Task, TaskList, TimeBlock,
    engine::{Error, Result},
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(crate) const HISTORY_LIMIT: u32 = 1000;
const APPLICATION_ID: i64 = 0x5554_4b53;
const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Entity {
    Task(Task),
    List(TaskList),
    Event(Event),
    TimeBlock(TimeBlock),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub entity: Entity,
    pub deleted: bool,
}

impl Snapshot {
    pub fn id(&self) -> EntityId {
        match &self.entity {
            Entity::Task(v) => EntityId::Task(v.id),
            Entity::List(v) => EntityId::List(v.id),
            Entity::Event(v) => EntityId::Event(v.id),
            Entity::TimeBlock(v) => EntityId::TimeBlock(v.id),
        }
    }
    pub fn revision(&self) -> u64 {
        match &self.entity {
            Entity::Task(v) => v.revision,
            Entity::List(v) => v.revision,
            Entity::Event(v) => v.revision,
            Entity::TimeBlock(v) => v.revision,
        }
    }
    pub fn set_revision(&mut self, revision: u64) {
        match &mut self.entity {
            Entity::Task(v) => v.revision = revision,
            Entity::List(v) => v.revision = revision,
            Entity::Event(v) => v.revision = revision,
            Entity::TimeBlock(v) => v.revision = revision,
        }
    }
    pub fn same_value(&self, other: &Self) -> bool {
        let mut a = self.clone();
        let mut b = other.clone();
        a.set_revision(0);
        b.set_revision(0);
        a == b
    }
}

pub(crate) fn key(id: &EntityId) -> (&'static str, String) {
    match id {
        EntityId::Task(v) => ("task", v.to_string()),
        EntityId::List(v) => ("list", v.to_string()),
        EntityId::Event(v) => ("event", v.to_string()),
        EntityId::TimeBlock(v) => ("block", v.to_string()),
    }
}

pub(crate) fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub(crate) fn initialize(connection: &mut Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let app_id: i64 = tx.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let objects: i64 = tx.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    let empty = version == 0 && app_id == 0 && objects == 0;
    if !empty && app_id != APPLICATION_ID {
        return Err(Error::UnrecognizedDatabase);
    }
    if version > SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema(version));
    }
    if !empty && !(1..=SCHEMA_VERSION).contains(&version) {
        return Err(Error::UnrecognizedDatabase);
    }
    if empty {
        tx.execute_batch("CREATE TABLE task_lists (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1))
        );
        CREATE TABLE tasks (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, completed INTEGER NOT NULL CHECK(completed IN (0,1)), due_json TEXT,
          list_id TEXT REFERENCES task_lists(id), revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1))
        );
        CREATE INDEX tasks_active ON tasks(deleted,completed,id);
        CREATE INDEX tasks_list ON tasks(list_id,deleted,id);
        CREATE TABLE events (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, schedule_json TEXT NOT NULL,
          start_utc TEXT, end_utc TEXT, start_date TEXT, end_date TEXT,
          revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1)),
          CHECK((start_utc IS NOT NULL AND end_utc IS NOT NULL AND start_date IS NULL AND end_date IS NULL) OR
                (start_utc IS NULL AND end_utc IS NULL AND start_date IS NOT NULL AND end_date IS NOT NULL))
        );
        CREATE INDEX events_timed ON events(deleted,start_utc,end_utc);
        CREATE INDEX events_all_day ON events(deleted,start_date,end_date);
        CREATE TABLE time_blocks (
          id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id), start_utc TEXT NOT NULL, end_utc TEXT NOT NULL,
          timezone TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), deleted INTEGER NOT NULL CHECK(deleted IN (0,1)), CHECK(start_utc<end_utc)
        );
        CREATE INDEX blocks_window ON time_blocks(deleted,start_utc,end_utc);
        CREATE INDEX blocks_task ON time_blocks(task_id,deleted);
        CREATE TABLE operations (
          seq INTEGER PRIMARY KEY AUTOINCREMENT, actor TEXT NOT NULL, before_json TEXT NOT NULL, after_json TEXT NOT NULL,
          undone INTEGER NOT NULL DEFAULT 0 CHECK(undone IN (0,1))
        );
        CREATE INDEX operations_actor ON operations(actor,undone,seq);
        CREATE TABLE history_guards (
          actor TEXT NOT NULL, kind TEXT NOT NULL, entity_id TEXT NOT NULL, revision INTEGER NOT NULL,
          PRIMARY KEY(actor,kind,entity_id)
        );
        PRAGMA user_version=1;")?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    } else {
        // Marker and version alone are not enough to trust a partial/damaged schema.
        for sql in [
            "SELECT id,title,revision,deleted FROM task_lists LIMIT 0",
            "SELECT id,title,completed,due_json,list_id,revision,deleted FROM tasks LIMIT 0",
            "SELECT id,title,schedule_json,start_utc,end_utc,start_date,end_date,revision,deleted FROM events LIMIT 0",
            "SELECT id,task_id,start_utc,end_utc,timezone,revision,deleted FROM time_blocks LIMIT 0",
            "SELECT seq,actor,before_json,after_json,undone FROM operations LIMIT 0",
            "SELECT actor,kind,entity_id,revision FROM history_guards LIMIT 0",
        ] {
            tx.prepare(sql).map_err(|_| Error::UnrecognizedDatabase)?;
        }
    }
    if version < 2 {
        // The default task page has no completed filter. This index preserves ID
        // order while filtering live rows, avoiding a sort of every matching task.
        tx.execute_batch("CREATE INDEX tasks_order ON tasks(deleted,id); PRAGMA user_version=2;")?;
    }
    tx.commit()?;
    // Journal conversion only happens after ownership/schema checks have succeeded.
    connection.pragma_update(None, "journal_mode", "WAL")?;
    Ok(())
}

fn decode_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
fn sql_revision(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::InvalidInput("revision exceeds SQLite integer range".into()))
}
fn read_revision(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(column)?).map_err(decode_error)
}
fn parse<T: std::str::FromStr>(value: String) -> rusqlite::Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value.parse().map_err(decode_error)
}
fn json<T: serde::de::DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(decode_error)
}

pub(crate) fn read_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let list: Option<String> = row.get(4)?;
    let due: Option<String> = row.get(3)?;
    Ok(Task {
        id: parse(row.get(0)?)?,
        title: row.get(1)?,
        completed: row.get(2)?,
        due: due.map(json).transpose()?,
        list_id: list.map(parse).transpose()?,
        revision: read_revision(row, 5)?,
    })
}
pub(crate) fn read_list(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskList> {
    Ok(TaskList {
        id: parse(row.get(0)?)?,
        title: row.get(1)?,
        revision: read_revision(row, 2)?,
    })
}
pub(crate) fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: parse(row.get(0)?)?,
        title: row.get(1)?,
        schedule: json(row.get(2)?)?,
        revision: read_revision(row, 3)?,
    })
}
pub(crate) fn read_block(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimeBlock> {
    Ok(TimeBlock {
        id: parse(row.get(0)?)?,
        task_id: parse(row.get(1)?)?,
        start: parse(row.get(2)?)?,
        end: parse(row.get(3)?)?,
        timezone: row.get(4)?,
        revision: read_revision(row, 5)?,
    })
}

pub(crate) fn get(connection: &Connection, id: &EntityId) -> Result<Option<Snapshot>> {
    let (_, id_string) = key(id);
    let value=match id {
        EntityId::Task(_)=>connection.query_row("SELECT id,title,completed,due_json,list_id,revision,deleted FROM tasks WHERE id=?1",[id_string],|row|Ok(Snapshot{entity:Entity::Task(read_task(row)?),deleted:row.get(6)?})).optional()?,
        EntityId::List(_)=>connection.query_row("SELECT id,title,revision,deleted FROM task_lists WHERE id=?1",[id_string],|row|Ok(Snapshot{entity:Entity::List(read_list(row)?),deleted:row.get(3)?})).optional()?,
        EntityId::Event(_)=>connection.query_row("SELECT id,title,schedule_json,revision,deleted FROM events WHERE id=?1",[id_string],|row|Ok(Snapshot{entity:Entity::Event(read_event(row)?),deleted:row.get(4)?})).optional()?,
        EntityId::TimeBlock(_)=>connection.query_row("SELECT id,task_id,start_utc,end_utc,timezone,revision,deleted FROM time_blocks WHERE id=?1",[id_string],|row|Ok(Snapshot{entity:Entity::TimeBlock(read_block(row)?),deleted:row.get(6)?})).optional()?,
    };
    Ok(value)
}

pub(crate) fn active(connection: &Connection, id: &EntityId) -> Result<Snapshot> {
    get(connection, id)?
        .filter(|v| !v.deleted)
        .ok_or_else(|| Error::NotFound(id.clone()))
}

pub(crate) fn put(connection: &Connection, snapshot: &Snapshot) -> Result<()> {
    let deleted = snapshot.deleted;
    match &snapshot.entity {
        Entity::List(v) => {
            connection.execute("INSERT INTO task_lists(id,title,revision,deleted) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET title=excluded.title,revision=excluded.revision,deleted=excluded.deleted",params![v.id.to_string(),v.title,sql_revision(v.revision)?,deleted])?;
        }
        Entity::Task(v) => {
            connection.execute("INSERT INTO tasks(id,title,completed,due_json,list_id,revision,deleted) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET title=excluded.title,completed=excluded.completed,due_json=excluded.due_json,list_id=excluded.list_id,revision=excluded.revision,deleted=excluded.deleted",params![v.id.to_string(),v.title,v.completed,v.due.as_ref().map(serde_json::to_string).transpose()?,v.list_id.map(|id|id.to_string()),sql_revision(v.revision)?,deleted])?;
        }
        Entity::Event(v) => {
            let (start_utc, end_utc, start_date, end_date) = match &v.schedule {
                EventSchedule::Timed { start, end, .. } => {
                    (Some(timestamp(*start)), Some(timestamp(*end)), None, None)
                }
                EventSchedule::AllDay { start, end } => {
                    (None, None, Some(start.to_string()), Some(end.to_string()))
                }
            };
            connection.execute("INSERT INTO events(id,title,schedule_json,start_utc,end_utc,start_date,end_date,revision,deleted) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET title=excluded.title,schedule_json=excluded.schedule_json,start_utc=excluded.start_utc,end_utc=excluded.end_utc,start_date=excluded.start_date,end_date=excluded.end_date,revision=excluded.revision,deleted=excluded.deleted",params![v.id.to_string(),v.title,serde_json::to_string(&v.schedule)?,start_utc,end_utc,start_date,end_date,sql_revision(v.revision)?,deleted])?;
        }
        Entity::TimeBlock(v) => {
            connection.execute("INSERT INTO time_blocks(id,task_id,start_utc,end_utc,timezone,revision,deleted) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET task_id=excluded.task_id,start_utc=excluded.start_utc,end_utc=excluded.end_utc,timezone=excluded.timezone,revision=excluded.revision,deleted=excluded.deleted",params![v.id.to_string(),v.task_id.to_string(),timestamp(v.start),timestamp(v.end),v.timezone,sql_revision(v.revision)?,deleted])?;
        }
    }
    Ok(())
}

pub(crate) fn task_blocks(
    connection: &Connection,
    task_id: crate::TaskId,
) -> Result<Vec<Snapshot>> {
    let mut statement=connection.prepare("SELECT id,task_id,start_utc,end_utc,timezone,revision FROM time_blocks WHERE task_id=?1 AND deleted=0 ORDER BY id")?;
    let rows = statement.query_map([task_id.to_string()], |row| {
        Ok(Snapshot {
            entity: Entity::TimeBlock(read_block(row)?),
            deleted: false,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn list_has_tasks(connection: &Connection, list_id: ListId) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE list_id=?1 AND deleted=0)",
        [list_id.to_string()],
        |row| row.get(0),
    )?)
}

pub(crate) fn guard(connection: &Connection, actor: &str, id: &EntityId) -> Result<Option<u64>> {
    let (kind, id) = key(id);
    Ok(connection
        .query_row(
            "SELECT revision FROM history_guards WHERE actor=?1 AND kind=?2 AND entity_id=?3",
            params![actor, kind, id],
            |row| read_revision(row, 0),
        )
        .optional()?)
}

pub(crate) fn set_guard(connection: &Connection, actor: &str, snapshot: &Snapshot) -> Result<()> {
    let (kind, id) = key(&snapshot.id());
    connection.execute("INSERT INTO history_guards(actor,kind,entity_id,revision) VALUES(?1,?2,?3,?4) ON CONFLICT(actor,kind,entity_id) DO UPDATE SET revision=excluded.revision",params![actor,kind,id,sql_revision(snapshot.revision())?])?;
    Ok(())
}
