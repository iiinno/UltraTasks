use crate::{
    model::*,
    storage::{self, Entity, Snapshot},
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{collections::HashMap, path::Path};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("entity not found: {0:?}")]
    NotFound(EntityId),
    #[error("revision conflict for {entity:?}: expected {expected}, actual {actual}")]
    Conflict {
        entity: EntityId,
        expected: u64,
        actual: u64,
    },
    #[error("history conflicts with newer data or a dependent entity: {0:?}")]
    HistoryConflict(EntityId),
    #[error("there is no operation to undo or redo")]
    NoHistory,
    #[error("list still contains active tasks")]
    ListNotEmpty,
    #[error("unsupported database schema version {0}")]
    UnsupportedSchema(i64),
    #[error("database is not a recognized UltraTasks database")]
    UnrecognizedDatabase,
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid stored data: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Synchronous, transactional engine. Create and use it on a worker thread.
/// The actor is a durable history stream identifier, not an authentication mechanism.
pub struct Engine {
    connection: Connection,
    actor: String,
}

impl Engine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_for_actor(path, "local")
    }

    pub fn open_for_actor(path: impl AsRef<Path>, actor: impl Into<String>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?, actor.into())
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?, "local".into())
    }

    fn from_connection(mut connection: Connection, actor: String) -> Result<Self> {
        if actor.is_empty() || actor.len() > 128 || actor.chars().any(char::is_control) {
            return Err(invalid(
                "actor must be 1..=128 bytes without control characters",
            ));
        }
        storage::initialize(&mut connection)?;
        Ok(Self { connection, actor })
    }

    pub fn execute(&mut self, command: Command) -> Result<Outcome> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (before, after, outcome) = apply_command(&tx, command)?;
        check_relations(&tx, &after, false)?;
        for snapshot in &after {
            storage::put(&tx, snapshot)?;
            storage::set_guard(&tx, &self.actor, snapshot)?;
        }
        // A fresh action starts a new history branch for this actor only.
        tx.execute(
            "DELETE FROM operations WHERE actor=?1 AND undone=1",
            [&self.actor],
        )?;
        tx.execute(
            "INSERT INTO operations(actor,before_json,after_json) VALUES(?1,?2,?3)",
            params![
                self.actor,
                serde_json::to_string(&before)?,
                serde_json::to_string(&after)?
            ],
        )?;
        tx.execute("DELETE FROM operations WHERE actor=?1 AND seq NOT IN (SELECT seq FROM operations WHERE actor=?1 ORDER BY seq DESC LIMIT ?2)",params![self.actor,storage::HISTORY_LIMIT])?;
        tx.commit()?;
        Ok(outcome)
    }

    pub fn task(&self, id: TaskId) -> Result<Task> {
        match storage::active(&self.connection, &EntityId::Task(id))?.entity {
            Entity::Task(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn task_list(&self, id: ListId) -> Result<TaskList> {
        match storage::active(&self.connection, &EntityId::List(id))?.entity {
            Entity::List(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn event(&self, id: EventId) -> Result<Event> {
        match storage::active(&self.connection, &EntityId::Event(id))?.entity {
            Entity::Event(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn time_block(&self, id: TimeBlockId) -> Result<TimeBlock> {
        match storage::active(&self.connection, &EntityId::TimeBlock(id))?.entity {
            Entity::TimeBlock(v) => Ok(v),
            _ => unreachable!(),
        }
    }

    pub fn tasks(&self, query: &TaskQuery) -> Result<Vec<Task>> {
        limit(query.limit)?;
        if query
            .search
            .as_ref()
            .is_some_and(|s| s.chars().count() > 1000)
        {
            return Err(invalid("search is too long"));
        }
        let mut statement=self.connection.prepare_cached("SELECT id,title,completed,due_json,list_id,revision FROM tasks WHERE deleted=0 AND (?1 IS NULL OR instr(lower(title),lower(?1))>0) AND (?2 IS NULL OR completed=?2) AND (?3 IS NULL OR list_id=?3) ORDER BY id LIMIT ?4 OFFSET ?5")?;
        let rows = statement.query_map(
            params![
                query.search,
                query.completed,
                query.list_id.map(|id| id.to_string()),
                query.limit,
                query.offset
            ],
            storage::read_task,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn lists(&self, count: u32) -> Result<Vec<TaskList>> {
        limit(count)?;
        let mut statement = self.connection.prepare_cached(
            "SELECT id,title,revision FROM task_lists WHERE deleted=0 ORDER BY id LIMIT ?1",
        )?;
        let rows = statement.query_map([count], storage::read_list)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn events(&self, query: &CalendarQuery) -> Result<Vec<Event>> {
        let (date_start, date_end) = calendar_window(query)?;
        let mut statement=self.connection.prepare_cached("SELECT id,title,schedule_json,revision FROM events WHERE deleted=0 AND ((start_utc<?2 AND end_utc>?1) OR (start_date<?4 AND end_date>?3)) ORDER BY COALESCE(start_utc,start_date),id LIMIT ?5")?;
        let rows = statement.query_map(
            params![
                storage::timestamp(query.start),
                storage::timestamp(query.end),
                date_start.to_string(),
                date_end.to_string(),
                query.limit
            ],
            storage::read_event,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn time_blocks(&self, query: &CalendarQuery) -> Result<Vec<TimeBlock>> {
        calendar_window(query)?;
        let mut statement=self.connection.prepare_cached("SELECT id,task_id,start_utc,end_utc,timezone,revision FROM time_blocks WHERE deleted=0 AND start_utc<?2 AND end_utc>?1 ORDER BY start_utc,id LIMIT ?3")?;
        let rows = statement.query_map(
            params![
                storage::timestamp(query.start),
                storage::timestamp(query.end),
                query.limit
            ],
            storage::read_block,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// At most the latest 1000 operations are retained per actor. Availability does not
    /// guarantee success: a concurrent actor can make an operation conflict.
    pub fn history_status(&self) -> Result<HistoryStatus> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM operations WHERE actor=?1 AND undone=0), EXISTS(SELECT 1 FROM operations WHERE actor=?1 AND undone=1)",[&self.actor],|row|Ok(HistoryStatus{can_undo:row.get(0)?,can_redo:row.get(1)?}))?)
    }

    pub fn undo(&mut self) -> Result<Vec<EntityId>> {
        self.replay(false)
    }
    pub fn redo(&mut self) -> Result<Vec<EntityId>> {
        self.replay(true)
    }

    fn replay(&mut self, redo: bool) -> Result<Vec<EntityId>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = if redo {
            "SELECT seq,before_json,after_json FROM operations WHERE actor=?1 AND undone=1 ORDER BY seq ASC LIMIT 1"
        } else {
            "SELECT seq,before_json,after_json FROM operations WHERE actor=?1 AND undone=0 ORDER BY seq DESC LIMIT 1"
        };
        let entry: Option<(i64, String, String)> = tx
            .query_row(sql, [&self.actor], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?;
        let (seq, before_json, after_json) = entry.ok_or(Error::NoHistory)?;
        let before: Vec<Snapshot> = serde_json::from_str(&before_json)?;
        let after: Vec<Snapshot> = serde_json::from_str(&after_json)?;
        let (expected, mut desired) = if redo {
            (before, after)
        } else {
            (after, before)
        };
        let original_boundary = desired.clone();
        if expected.len() != desired.len() {
            return Err(invalid("stored history has mismatched entity counts"));
        }
        for (expected, target) in expected.iter().zip(&mut desired) {
            let id = expected.id();
            if target.id() != id {
                return Err(invalid("stored history has mismatched entity identifiers"));
            }
            let current =
                storage::get(&tx, &id)?.ok_or_else(|| Error::HistoryConflict(id.clone()))?;
            if storage::guard(&tx, &self.actor, &id)? != Some(current.revision())
                || current.revision() != expected.revision()
                || !current.same_value(expected)
            {
                return Err(Error::HistoryConflict(id));
            }
            target.set_revision(next_revision(current.revision())?);
        }
        check_relations(&tx, &desired, true)?;
        for snapshot in &desired {
            storage::put(&tx, snapshot)?;
            storage::set_guard(&tx, &self.actor, snapshot)?;
        }
        let sql = if redo {
            "UPDATE operations SET undone=0,after_json=?2 WHERE seq=?1"
        } else {
            "UPDATE operations SET undone=1,before_json=?2 WHERE seq=?1"
        };
        tx.execute(sql, params![seq, serde_json::to_string(&desired)?])?;
        rebase_history_boundary(&tx, &self.actor, seq, redo, &original_boundary, &desired)?;
        tx.commit()?;
        Ok(desired.iter().map(Snapshot::id).collect())
    }
}

/// Only rebase the nearest touching operation if it is the exact revision boundary
/// being restored. An intervening foreign actor write remains a permanent barrier,
/// even if it wrote identical field values (the ABA case).
fn rebase_history_boundary(
    connection: &Connection,
    actor: &str,
    seq: i64,
    redo: bool,
    original: &[Snapshot],
    restored: &[Snapshot],
) -> Result<()> {
    let mut pending: HashMap<EntityId, (&Snapshot, &Snapshot)> = original
        .iter()
        .zip(restored)
        .map(|(old, new)| (old.id(), (old, new)))
        .collect();
    let sql = if redo {
        "SELECT seq,before_json FROM operations WHERE actor=?1 AND seq>?2 AND undone=1 ORDER BY seq ASC"
    } else {
        "SELECT seq,after_json FROM operations WHERE actor=?1 AND seq<?2 AND undone=0 ORDER BY seq DESC"
    };
    let mut statement = connection.prepare(sql)?;
    let entries = statement
        .query_map(params![actor, seq], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (neighbor_seq, json) in entries {
        if pending.is_empty() {
            break;
        }
        let mut boundary: Vec<Snapshot> = serde_json::from_str(&json)?;
        let mut changed = false;
        for value in &mut boundary {
            if let Some((old, new)) = pending.remove(&value.id())
                && value.revision() == old.revision()
                && value.same_value(old)
            {
                value.set_revision(new.revision());
                changed = true;
            }
        }
        if changed {
            let sql = if redo {
                "UPDATE operations SET before_json=?2 WHERE seq=?1"
            } else {
                "UPDATE operations SET after_json=?2 WHERE seq=?1"
            };
            connection.execute(
                sql,
                params![neighbor_seq, serde_json::to_string(&boundary)?],
            )?;
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidInput(message.into())
}
fn limit(value: u32) -> Result<()> {
    if !(1..=1000).contains(&value) {
        return Err(invalid("query limit must be 1..=1000"));
    }
    Ok(())
}
fn title(value: String) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 1000 || value.chars().any(char::is_control) {
        return Err(invalid(
            "title must contain 1..=1000 characters without control characters",
        ));
    }
    Ok(value.to_owned())
}
fn timezone(value: &str) -> Result<Tz> {
    value
        .parse()
        .map_err(|_| invalid("timezone must be a valid IANA timezone"))
}
fn date(value: NaiveDate) -> Result<()> {
    if !(1..=9999).contains(&value.year()) {
        return Err(invalid("date year must be 1..=9999"));
    }
    Ok(())
}
fn instant(value: DateTime<Utc>, zone: Tz) -> Result<()> {
    date(value.date_naive())?;
    date(value.with_timezone(&zone).date_naive())?;
    if value.timestamp_subsec_nanos() >= 1_000_000_000 {
        return Err(invalid("leap-second timestamps are not supported"));
    }
    Ok(())
}
fn timed_range(start: DateTime<Utc>, end: DateTime<Utc>, zone: &str) -> Result<()> {
    let zone = timezone(zone)?;
    instant(start, zone)?;
    instant(end, zone)?;
    if end <= start {
        return Err(invalid("end must be later than start"));
    }
    Ok(())
}
fn due(value: &Option<DueDate>) -> Result<()> {
    match value {
        Some(DueDate::Date(d)) => date(*d),
        Some(DueDate::At { at, timezone: zone }) => instant(*at, timezone(zone)?),
        None => Ok(()),
    }
}
fn schedule(value: &EventSchedule) -> Result<()> {
    match value {
        EventSchedule::Timed {
            start,
            end,
            timezone,
        } => timed_range(*start, *end, timezone),
        EventSchedule::AllDay { start, end } => {
            date(*start)?;
            date(*end)?;
            if end <= start {
                return Err(invalid("all-day end date must be later than start date"));
            }
            Ok(())
        }
    }
}
fn calendar_window(query: &CalendarQuery) -> Result<(NaiveDate, NaiveDate)> {
    limit(query.limit)?;
    timed_range(query.start, query.end, &query.timezone)?;
    if query.end - query.start > chrono::Duration::days(366) {
        return Err(invalid("calendar query window must not exceed 366 days"));
    }
    let zone = timezone(&query.timezone)?;
    let start = query.start.with_timezone(&zone).date_naive();
    let local_end = query.end.with_timezone(&zone);
    let end = if local_end.time() == NaiveTime::MIN {
        local_end.date_naive()
    } else {
        local_end
            .date_naive()
            .succ_opt()
            .ok_or_else(|| invalid("calendar query end overflows"))?
    };
    date(start)?;
    date(end)?;
    Ok((start, end))
}
fn next_revision(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| invalid("entity revision exhausted"))
}
fn checked(connection: &Connection, id: EntityId, expected: u64) -> Result<Snapshot> {
    let value = storage::active(connection, &id)?;
    if value.revision() != expected {
        return Err(Error::Conflict {
            entity: id,
            expected,
            actual: value.revision(),
        });
    }
    Ok(value)
}
fn created(entity: Entity, outcome: Outcome) -> (Vec<Snapshot>, Vec<Snapshot>, Outcome) {
    let after = Snapshot {
        entity,
        deleted: false,
    };
    let mut before = after.clone();
    before.deleted = true;
    before.set_revision(0);
    (vec![before], vec![after], outcome)
}
fn updated(
    before: Snapshot,
    mut after: Snapshot,
    outcome: impl FnOnce(&Entity) -> Outcome,
) -> Result<(Vec<Snapshot>, Vec<Snapshot>, Outcome)> {
    after.set_revision(next_revision(before.revision())?);
    let outcome = outcome(&after.entity);
    Ok((vec![before], vec![after], outcome))
}
fn outcome(entity: &Entity) -> Outcome {
    match entity {
        Entity::Task(v) => Outcome::Task(v.clone()),
        Entity::List(v) => Outcome::List(v.clone()),
        Entity::Event(v) => Outcome::Event(v.clone()),
        Entity::TimeBlock(v) => Outcome::TimeBlock(v.clone()),
    }
}

fn apply_command(
    connection: &Connection,
    command: Command,
) -> Result<(Vec<Snapshot>, Vec<Snapshot>, Outcome)> {
    match command {
        Command::CreateList { title: value } => {
            let v = TaskList {
                id: ListId::new(),
                title: title(value)?,
                revision: 1,
            };
            Ok(created(Entity::List(v.clone()), Outcome::List(v)))
        }
        Command::RenameList {
            id,
            expected_revision,
            title: value,
        } => {
            let before = checked(connection, EntityId::List(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::List(v) = &mut after.entity {
                v.title = title(value)?;
            }
            updated(before, after, outcome)
        }
        Command::DeleteList {
            id,
            expected_revision,
        } => {
            let before = checked(connection, EntityId::List(id), expected_revision)?;
            if storage::list_has_tasks(connection, id)? {
                return Err(Error::ListNotEmpty);
            }
            let mut after = before.clone();
            after.deleted = true;
            updated(before, after, |_| Outcome::Deleted(EntityId::List(id)))
        }
        Command::CreateTask {
            title: value,
            due: value_due,
            list_id,
        } => {
            due(&value_due)?;
            let v = Task {
                id: TaskId::new(),
                title: title(value)?,
                completed: false,
                due: value_due,
                list_id,
                revision: 1,
            };
            Ok(created(Entity::Task(v.clone()), Outcome::Task(v)))
        }
        Command::UpdateTask {
            id,
            expected_revision,
            title: value,
            completed,
            due: value_due,
            list_id,
        } => {
            let before = checked(connection, EntityId::Task(id), expected_revision)?;
            due(&value_due)?;
            let after = Snapshot {
                entity: Entity::Task(Task {
                    id,
                    title: title(value)?,
                    completed,
                    due: value_due,
                    list_id,
                    revision: 0,
                }),
                deleted: false,
            };
            updated(before, after, outcome)
        }
        Command::SetTaskCompleted {
            id,
            expected_revision,
            completed,
        } => {
            let before = checked(connection, EntityId::Task(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Task(v) = &mut after.entity {
                v.completed = completed;
            }
            updated(before, after, outcome)
        }
        Command::DeleteTask {
            id,
            expected_revision,
        } => {
            let task = checked(connection, EntityId::Task(id), expected_revision)?;
            let mut before = vec![task];
            before.extend(storage::task_blocks(connection, id)?);
            let mut after = before.clone();
            for v in &mut after {
                v.deleted = true;
                v.set_revision(next_revision(v.revision())?);
            }
            Ok((before, after, Outcome::Deleted(EntityId::Task(id))))
        }
        Command::CreateEvent {
            title: value,
            schedule: value_schedule,
        } => {
            schedule(&value_schedule)?;
            let v = Event {
                id: EventId::new(),
                title: title(value)?,
                schedule: value_schedule,
                revision: 1,
            };
            Ok(created(Entity::Event(v.clone()), Outcome::Event(v)))
        }
        Command::UpdateEvent {
            id,
            expected_revision,
            title: value,
            schedule: value_schedule,
        } => {
            let before = checked(connection, EntityId::Event(id), expected_revision)?;
            schedule(&value_schedule)?;
            let after = Snapshot {
                entity: Entity::Event(Event {
                    id,
                    title: title(value)?,
                    schedule: value_schedule,
                    revision: 0,
                }),
                deleted: false,
            };
            updated(before, after, outcome)
        }
        Command::DeleteEvent {
            id,
            expected_revision,
        } => {
            let before = checked(connection, EntityId::Event(id), expected_revision)?;
            let mut after = before.clone();
            after.deleted = true;
            updated(before, after, |_| Outcome::Deleted(EntityId::Event(id)))
        }
        Command::ScheduleTask {
            task_id,
            expected_task_revision,
            start,
            end,
            timezone,
        } => {
            checked(connection, EntityId::Task(task_id), expected_task_revision)?;
            timed_range(start, end, &timezone)?;
            let v = TimeBlock {
                id: TimeBlockId::new(),
                task_id,
                start,
                end,
                timezone,
                revision: 1,
            };
            Ok(created(Entity::TimeBlock(v.clone()), Outcome::TimeBlock(v)))
        }
        Command::UpdateTimeBlock {
            id,
            expected_revision,
            start,
            end,
            timezone,
        } => {
            let before = checked(connection, EntityId::TimeBlock(id), expected_revision)?;
            timed_range(start, end, &timezone)?;
            let mut after = before.clone();
            if let Entity::TimeBlock(v) = &mut after.entity {
                v.start = start;
                v.end = end;
                v.timezone = timezone;
            }
            updated(before, after, outcome)
        }
        Command::DeleteTimeBlock {
            id,
            expected_revision,
        } => {
            let before = checked(connection, EntityId::TimeBlock(id), expected_revision)?;
            let mut after = before.clone();
            after.deleted = true;
            updated(before, after, |_| Outcome::Deleted(EntityId::TimeBlock(id)))
        }
    }
}

/// Validate the final relation graph before applying any row. This also protects undo
/// from deleting a parent that gained a child in another actor's history stream.
fn check_relations(connection: &Connection, targets: &[Snapshot], history: bool) -> Result<()> {
    let target_map: HashMap<EntityId, &Snapshot> = targets.iter().map(|v| (v.id(), v)).collect();
    let is_active = |id: &EntityId| -> Result<bool> {
        if let Some(value) = target_map.get(id) {
            return Ok(!value.deleted);
        }
        Ok(storage::get(connection, id)?.is_some_and(|v| !v.deleted))
    };
    let relation_error = |id: EntityId| {
        if history {
            Error::HistoryConflict(id)
        } else {
            Error::NotFound(id)
        }
    };
    for value in targets {
        match &value.entity {
            Entity::Task(task) if !value.deleted => {
                if let Some(list) = task.list_id {
                    let id = EntityId::List(list);
                    if !is_active(&id)? {
                        return Err(relation_error(id));
                    }
                }
            }
            Entity::TimeBlock(block) if !value.deleted => {
                let id = EntityId::Task(block.task_id);
                if !is_active(&id)? {
                    return Err(relation_error(id));
                }
            }
            Entity::Task(task) if value.deleted => {
                for block in storage::task_blocks(connection, task.id)? {
                    if is_active(&block.id())? {
                        return Err(relation_error(value.id()));
                    }
                }
            }
            Entity::List(list) if value.deleted => {
                let mut statement =
                    connection.prepare("SELECT id FROM tasks WHERE list_id=?1 AND deleted=0")?;
                let ids = statement
                    .query_map([list.id.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for id in ids {
                    let id = id
                        .parse::<TaskId>()
                        .map_err(|_| invalid("invalid stored task ID"))?;
                    if is_active(&EntityId::Task(id))? {
                        return Err(if history {
                            Error::HistoryConflict(value.id())
                        } else {
                            Error::ListNotEmpty
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
