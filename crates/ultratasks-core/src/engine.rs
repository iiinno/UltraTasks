use crate::{
    model::*,
    storage::{self, Entity, Snapshot},
};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

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
    pub(crate) connection: Connection,
    pub(crate) actor: String,
}

impl Engine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_for_actor(path, "local")
    }

    /// Read an existing current-schema database without creating, migrating or
    /// changing its journal mode. SQLite itself rejects writes on this handle.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        storage::validate_read_only(&connection)?;
        connection.pragma_update(None, "query_only", true)?;
        Ok(Self {
            connection,
            actor: "readonly".into(),
        })
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
        crate::sync::initialize(&mut connection)?;
        Ok(Self { connection, actor })
    }

    pub fn execute(&mut self, command: Command) -> Result<Outcome> {
        Ok(self.execute_batch(vec![command])?.remove(0))
    }

    /// Apply a bounded group atomically; the entire group is one undo step.
    pub fn execute_batch(&mut self, commands: Vec<Command>) -> Result<Vec<Outcome>> {
        self.run_commands(commands, None, false, &[], false)
    }

    /// Retry-safe request, scoped to the durable actor. Keys are retained until a
    /// full data export/import reset; reusing one with different input is an error.
    pub fn execute_idempotent(
        &mut self,
        request_id: &str,
        commands: Vec<Command>,
    ) -> Result<Vec<Outcome>> {
        if request_id.is_empty()
            || request_id.len() > 128
            || request_id.chars().any(char::is_control)
        {
            return Err(invalid("request ID must be 1..=128 bytes without controls"));
        }
        self.run_commands(commands, Some(request_id), false, &[], false)
    }

    /// Run the exact command/constraint path, then roll back all effects.
    pub fn preview_batch(&mut self, commands: Vec<Command>) -> Result<Vec<Outcome>> {
        self.run_commands(commands, None, true, &[], false)
    }

    pub fn execute_idempotent_guarded(
        &mut self,
        request_id: &str,
        commands: Vec<Command>,
        guards: &[(EntityId, u64)],
    ) -> Result<Vec<Outcome>> {
        if request_id.is_empty()
            || request_id.len() > 128
            || request_id.chars().any(char::is_control)
        {
            return Err(invalid("request ID must be 1..=128 bytes without controls"));
        }
        if guards.len() > 1000 {
            return Err(invalid("too many revision guards"));
        }
        self.run_commands(commands, Some(request_id), false, guards, false)
    }

    /// Apply only within the selected existing entities, including implicit cascade
    /// edits. New dependent entities created after proposal validation are rejected.
    pub fn execute_idempotent_scoped(
        &mut self,
        request_id: &str,
        commands: Vec<Command>,
        guards: &[(EntityId, u64)],
    ) -> Result<Vec<Outcome>> {
        if request_id.is_empty()
            || request_id.len() > 128
            || request_id.chars().any(char::is_control)
        {
            return Err(invalid("request ID must be 1..=128 bytes without controls"));
        }
        self.run_commands(commands, Some(request_id), false, guards, true)
    }
    pub fn preview_batch_scoped(
        &mut self,
        commands: Vec<Command>,
        guards: &[(EntityId, u64)],
    ) -> Result<Vec<Outcome>> {
        self.run_commands(commands, None, true, guards, true)
    }

    fn run_commands(
        &mut self,
        commands: Vec<Command>,
        request_id: Option<&str>,
        preview: bool,
        guards: &[(EntityId, u64)],
        scoped: bool,
    ) -> Result<Vec<Outcome>> {
        if guards.len() > 1000 {
            return Err(invalid("too many revision guards"));
        }
        if commands.is_empty() || commands.len() > 1000 {
            return Err(invalid("batch must contain 1..=1000 commands"));
        }
        let commands_json = serde_json::to_string(&(&commands, guards, scoped))?;
        if commands_json.len() > 4 * 1024 * 1024 {
            return Err(invalid("command batch exceeds 4 MiB"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(id) = request_id {
            let saved: Option<(String,String)> = tx.query_row("SELECT commands_json,outcomes_json FROM requests WHERE actor=?1 AND request_id=?2",params![self.actor,id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
            if let Some((prior, outcomes)) = saved {
                if prior != commands_json {
                    return Err(invalid(
                        "request ID was already used with different commands",
                    ));
                }
                return Ok(serde_json::from_str(&outcomes)?);
            }
        }
        for (id, revision) in guards {
            checked(&tx, id.clone(), *revision)?;
        }
        let mut before = Vec::<Snapshot>::new();
        let mut after = Vec::<Snapshot>::new();
        let mut positions = HashMap::<EntityId, usize>::new();
        let mut outcomes = Vec::new();
        let allowed: HashSet<_> = guards.iter().map(|(id, _)| id.clone()).collect();
        let mut created = HashSet::new();
        for command in commands {
            let (old, new, outcome) = apply_command(&tx, command)?;
            for value in &old {
                if value.revision() == 0 {
                    created.insert(value.id());
                } else if scoped
                    && !value.deleted
                    && !created.contains(&value.id())
                    && !allowed.contains(&value.id())
                {
                    return Err(invalid(
                        "operation would change an existing entity outside the selected scope",
                    ));
                }
            }
            check_relations(&tx, &new, false)?;
            for (old, new) in old.into_iter().zip(new) {
                validate_snapshot(&new)?;
                storage::put(&tx, &new)?;
                if let Some(index) = positions.get(&new.id()).copied() {
                    after[index] = new;
                } else {
                    positions.insert(new.id(), before.len());
                    before.push(old);
                    after.push(new);
                }
            }
            outcomes.push(outcome);
        }
        write_history(&tx, &self.actor, &before, &after)?;
        if let Some(id) = request_id {
            tx.execute("INSERT INTO requests(actor,request_id,commands_json,outcomes_json) VALUES(?1,?2,?3,?4)",params![self.actor,id,commands_json,serde_json::to_string(&outcomes)?])?;
        }
        if preview {
            tx.rollback()?;
        } else {
            tx.commit()?;
        }
        Ok(outcomes)
    }

    pub fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let tx = self.connection.unchecked_transaction()?;
        let snapshots = storage::all_snapshots(&tx)?;
        tx.commit()?;
        Ok(snapshots)
    }

    pub fn calendar(&self, id: CalendarId) -> Result<Calendar> {
        match storage::active(&self.connection, &EntityId::Calendar(id))?.entity {
            Entity::Calendar(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn calendars(&self, count: u32) -> Result<Vec<Calendar>> {
        limit(count)?;
        let mut statement = self
            .connection
            .prepare("SELECT id FROM calendars WHERE deleted=0 ORDER BY id LIMIT ?1")?;
        let ids = statement
            .query_map([count], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .map(|id| {
                self.calendar(
                    id.parse()
                        .map_err(|_| invalid("invalid stored calendar ID"))?,
                )
            })
            .collect()
    }
    pub fn reminder(&self, id: ReminderId) -> Result<Reminder> {
        match storage::active(&self.connection, &EntityId::Reminder(id))?.entity {
            Entity::Reminder(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    pub fn reminders(&self, count: u32) -> Result<Vec<Reminder>> {
        self.reminders_page(count, 0)
    }
    pub fn reminders_page(&self, count: u32, offset: u32) -> Result<Vec<Reminder>> {
        limit(count)?;
        let mut statement = self.connection.prepare(
            "SELECT data_json FROM reminders WHERE deleted=0 ORDER BY id LIMIT ?1 OFFSET ?2",
        )?;
        let values = statement
            .query_map(params![count, offset], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
            .into_iter()
            .map(|value| Ok(serde_json::from_str(&value)?))
            .collect()
    }
    pub fn event_occurrences(
        &self,
        query: &CalendarQuery,
        calendar_id: Option<CalendarId>,
    ) -> Result<Vec<EventOccurrence>> {
        let (date_start, date_end) = calendar_window(query)?;
        let mut statement = self.connection.prepare("SELECT id,title,schedule_json,revision,calendar_id,recurrence_json,exceptions_json FROM events WHERE deleted=0 AND (?1 IS NULL OR calendar_id=?1) AND (recurrence_json IS NOT NULL OR (start_utc<?3 AND end_utc>?2) OR (start_date<?5 AND end_date>?4)) ORDER BY id")?;
        let events = statement
            .query_map(
                params![
                    calendar_id.map(|id| id.to_string()),
                    storage::timestamp(query.start),
                    storage::timestamp(query.end),
                    date_start.to_string(),
                    date_end.to_string()
                ],
                storage::read_event,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result = Vec::new();
        for event in events {
            result.extend(crate::recurrence::expand_event(&event, query)?);
            if result.len() > query.limit as usize {
                return Err(invalid("occurrence query exceeds limit"));
            }
        }
        result.sort_by_key(|v| match &v.schedule {
            EventSchedule::Timed { start, .. } => *start,
            EventSchedule::AllDay { start, .. } => start.and_hms_opt(0, 0, 0).unwrap().and_utc(),
        });
        Ok(result)
    }
    pub fn task_occurrences(&self, query: &CalendarQuery) -> Result<Vec<TaskOccurrence>> {
        calendar_window(query)?;
        let mut statement = self.connection.prepare("SELECT id,title,completed,due_json,list_id,revision,recurrence_json,exceptions_json FROM tasks WHERE deleted=0 AND due_json IS NOT NULL ORDER BY id")?;
        let tasks = statement
            .query_map([], storage::read_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result = Vec::new();
        for task in tasks {
            result.extend(crate::recurrence::expand_task(&task, query)?);
            if result.len() > query.limit as usize {
                return Err(invalid("occurrence query exceeds limit"));
            }
        }
        result.sort_by_key(|v| match &v.due {
            DueDate::At { at, .. } => *at,
            DueDate::Date(date) => date.and_hms_opt(0, 0, 0).unwrap().and_utc(),
        });
        Ok(result)
    }
    /// Filter before pagination. A date-only deadline is overdue only after its
    /// local calendar day has ended; its timezone comes explicitly from the query.
    pub fn tasks_due(&self, query: &TaskQuery, filter: &DueFilter) -> Result<Vec<Task>> {
        // Every internal page must observe the same committed database state.
        let _read_transaction = self.connection.unchecked_transaction()?;
        limit(query.limit)?;
        let (start, end, zone) = match filter {
            DueFilter::None => (None, None, None),
            DueFilter::Range {
                start,
                end,
                timezone: zone,
            } => {
                timed_range(*start, *end, zone)?;
                (Some(*start), Some(*end), Some(timezone(zone)?))
            }
            DueFilter::Before { at, timezone: zone } => {
                let zone = timezone(zone)?;
                instant(*at, zone)?;
                (None, Some(*at), Some(zone))
            }
        };
        let mut scan = query.clone();
        scan.limit = 1000;
        scan.offset = 0;
        let mut matched = 0u32;
        let mut result = Vec::new();
        loop {
            let page = self.tasks(&scan)?;
            let count = page.len();
            for task in page {
                let matches = match (&task.due, filter) {
                    (None, DueFilter::None) => true,
                    (Some(DueDate::At { at, .. }), _) => {
                        end.is_some_and(|end| *at < end) && start.is_none_or(|start| *at >= start)
                    }
                    (Some(DueDate::Date(date)), DueFilter::Before { .. }) => end
                        .zip(zone)
                        .is_some_and(|(end, zone)| *date < end.with_timezone(&zone).date_naive()),
                    (Some(DueDate::Date(date)), DueFilter::Range { .. }) => {
                        let (start, end, zone) = (start.unwrap(), end.unwrap(), zone.unwrap());
                        let local_end = end.with_timezone(&zone);
                        let date_end = if local_end.time() == NaiveTime::MIN {
                            local_end.date_naive()
                        } else {
                            local_end
                                .date_naive()
                                .succ_opt()
                                .ok_or_else(|| invalid("due range date overflow"))?
                        };
                        *date >= start.with_timezone(&zone).date_naive() && *date < date_end
                    }
                    _ => false,
                };
                if matches {
                    if matched >= query.offset {
                        result.push(task);
                        if result.len() == query.limit as usize {
                            return Ok(result);
                        }
                    }
                    matched = matched
                        .checked_add(1)
                        .ok_or_else(|| invalid("task result offset overflow"))?;
                }
            }
            if count < 1000 {
                break;
            }
            scan.offset = scan
                .offset
                .checked_add(1000)
                .ok_or_else(|| invalid("task scan offset overflow"))?;
        }
        Ok(result)
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
        let mut statement=self.connection.prepare_cached("SELECT id,title,completed,due_json,list_id,revision,recurrence_json,exceptions_json FROM tasks WHERE deleted=0 AND (?1 IS NULL OR instr(lower(title),lower(?1))>0) AND (?2 IS NULL OR completed=?2) AND (?3 IS NULL OR list_id=?3) ORDER BY id LIMIT ?4 OFFSET ?5")?;
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
        let mut statement=self.connection.prepare_cached("SELECT id,title,schedule_json,revision,calendar_id,recurrence_json,exceptions_json FROM events WHERE deleted=0 AND ((start_utc<?2 AND end_utc>?1) OR (start_date<?4 AND end_date>?3)) ORDER BY COALESCE(start_utc,start_date),id LIMIT ?5")?;
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
            validate_snapshot(snapshot)?;
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
        crate::sync::record_local_change(&tx, &expected, &desired)?;
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
pub(crate) fn calendar_window(query: &CalendarQuery) -> Result<(NaiveDate, NaiveDate)> {
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
        Entity::Calendar(v) => Outcome::Calendar(v.clone()),
        Entity::Reminder(v) => Outcome::Reminder(v.clone()),
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
        Command::CreateCalendar { title: value } => {
            let v = Calendar {
                id: CalendarId::new(),
                title: title(value)?,
                revision: 1,
            };
            Ok(created(Entity::Calendar(v.clone()), Outcome::Calendar(v)))
        }
        Command::RenameCalendar {
            id,
            expected_revision,
            title: value,
        } => {
            let before = checked(connection, EntityId::Calendar(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Calendar(v) = &mut after.entity {
                v.title = title(value)?;
            }
            updated(before, after, outcome)
        }
        Command::DeleteCalendar {
            id,
            expected_revision,
        } => {
            let before = checked(connection, EntityId::Calendar(id), expected_revision)?;
            let mut after = before.clone();
            after.deleted = true;
            updated(before, after, |_| Outcome::Deleted(EntityId::Calendar(id)))
        }
        Command::AssignEventCalendar {
            id,
            expected_revision,
            calendar_id,
        } => {
            let before = checked(connection, EntityId::Event(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Event(v) = &mut after.entity {
                v.calendar_id = calendar_id;
            }
            updated(before, after, outcome)
        }
        Command::SetEventRecurrence {
            id,
            expected_revision,
            recurrence,
        } => {
            let before = checked(connection, EntityId::Event(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Event(v) = &mut after.entity {
                if !v.exceptions.is_empty() && v.recurrence != recurrence {
                    return Err(invalid(
                        "clear exceptions before changing a recurrence rule",
                    ));
                }
                v.recurrence = recurrence;
            }
            updated(before, after, outcome)
        }
        Command::SetEventException {
            id,
            expected_revision,
            date,
            exception,
        } => {
            let before = checked(connection, EntityId::Event(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Event(v) = &mut after.entity {
                if let Some(mut exception) = exception {
                    if let EventException::Override { title: value, .. } = &mut exception {
                        *value = value.take().map(title).transpose()?;
                    }
                    v.exceptions.insert(date, exception);
                } else {
                    v.exceptions.remove(&date);
                }
            }
            updated(before, after, outcome)
        }
        Command::SetTaskRecurrence {
            id,
            expected_revision,
            recurrence,
        } => {
            let before = checked(connection, EntityId::Task(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Task(v) = &mut after.entity {
                if !v.exceptions.is_empty() && v.recurrence != recurrence {
                    return Err(invalid(
                        "clear exceptions before changing a recurrence rule",
                    ));
                }
                v.recurrence = recurrence;
            }
            updated(before, after, outcome)
        }
        Command::SetTaskException {
            id,
            expected_revision,
            date,
            exception,
        } => {
            let before = checked(connection, EntityId::Task(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Task(v) = &mut after.entity {
                if let Some(mut exception) = exception {
                    if let TaskException::Override { title: value, .. } = &mut exception {
                        *value = value.take().map(title).transpose()?;
                    }
                    v.exceptions.insert(date, exception);
                } else {
                    v.exceptions.remove(&date);
                }
            }
            updated(before, after, outcome)
        }
        Command::CreateReminder { target, trigger } => {
            let v = Reminder {
                id: ReminderId::new(),
                target,
                trigger,
                enabled: true,
                revision: 1,
            };
            Ok(created(Entity::Reminder(v.clone()), Outcome::Reminder(v)))
        }
        Command::UpdateReminder {
            id,
            expected_revision,
            trigger,
            enabled,
        } => {
            let before = checked(connection, EntityId::Reminder(id), expected_revision)?;
            let mut after = before.clone();
            if let Entity::Reminder(v) = &mut after.entity {
                v.trigger = trigger;
                v.enabled = enabled;
            }
            updated(before, after, outcome)
        }
        Command::DeleteReminder {
            id,
            expected_revision,
        } => {
            let before = checked(connection, EntityId::Reminder(id), expected_revision)?;
            let mut after = before.clone();
            after.deleted = true;
            updated(before, after, |_| Outcome::Deleted(EntityId::Reminder(id)))
        }
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
                recurrence: None,
                exceptions: Default::default(),
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
            let mut after = before.clone();
            if let Entity::Task(v) = &mut after.entity {
                if v.due != value_due && !v.exceptions.is_empty() {
                    return Err(invalid(
                        "clear occurrence exceptions before changing the series anchor",
                    ));
                }
                v.title = title(value)?;
                v.completed = completed;
                v.due = value_due;
                v.list_id = list_id;
            }
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
            let targets: Vec<_> = before.iter().map(Snapshot::id).collect();
            before.extend(target_reminders(connection, &targets)?);
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
                calendar_id: None,
                recurrence: None,
                exceptions: Default::default(),
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
            let mut after = before.clone();
            if let Entity::Event(v) = &mut after.entity {
                if v.schedule != value_schedule && !v.exceptions.is_empty() {
                    return Err(invalid(
                        "clear occurrence exceptions before changing the series anchor",
                    ));
                }
                v.title = title(value)?;
                v.schedule = value_schedule;
            }
            updated(before, after, outcome)
        }
        Command::DeleteEvent {
            id,
            expected_revision,
        } => {
            let before = checked(connection, EntityId::Event(id), expected_revision)?;
            deleted_with_reminders(connection, before, EntityId::Event(id))
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
            deleted_with_reminders(connection, before, EntityId::TimeBlock(id))
        }
    }
}

/// Validate the final relation graph before applying any row. This also protects undo
/// from deleting a parent that gained a child in another actor's history stream.
pub(crate) fn check_relations(
    connection: &Connection,
    targets: &[Snapshot],
    history: bool,
) -> Result<()> {
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
    // Load reminder reverse edges once per graph validation, rather than scanning
    // all reminders again for each deleted entity in a backup/import/cascade.
    let deleted_targets: HashSet<_> = targets
        .iter()
        .filter(|v| {
            v.deleted
                && matches!(
                    v.entity,
                    Entity::Task(_) | Entity::Event(_) | Entity::TimeBlock(_)
                )
        })
        .map(Snapshot::id)
        .collect();
    let mut reminder_dependents = HashMap::<EntityId, Vec<EntityId>>::new();
    if !deleted_targets.is_empty() {
        let mut statement =
            connection.prepare("SELECT data_json FROM reminders WHERE deleted=0")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            let reminder: Reminder = serde_json::from_str(&row?)?;
            if deleted_targets.contains(&reminder.target) {
                reminder_dependents
                    .entry(reminder.target)
                    .or_default()
                    .push(EntityId::Reminder(reminder.id));
            }
        }
    }
    for value in targets {
        for dependency in snapshot_dependencies(value) {
            if !is_active(&dependency)? {
                return Err(relation_error(dependency));
            }
        }
        if value.deleted
            && let Some(reminders) = reminder_dependents.get(&value.id())
        {
            for reminder in reminders {
                if is_active(reminder)? {
                    return Err(relation_error(value.id()));
                }
            }
        }
        match &value.entity {
            Entity::Calendar(calendar) if value.deleted => {
                let mut statement = connection
                    .prepare("SELECT id FROM events WHERE calendar_id=?1 AND deleted=0")?;
                let ids = statement
                    .query_map([calendar.id.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for id in ids {
                    if is_active(&EntityId::Event(
                        id.parse().map_err(|_| invalid("invalid stored event ID"))?,
                    ))? {
                        return Err(relation_error(value.id()));
                    }
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

/// Shared durable history path for commands and imports. Caller owns the transaction.
pub(crate) fn write_history(
    connection: &Connection,
    actor: &str,
    before: &[Snapshot],
    after: &[Snapshot],
) -> Result<()> {
    crate::sync::record_local_change(connection, before, after)?;
    for snapshot in after {
        storage::set_guard(connection, actor, snapshot)?;
    }
    connection.execute(
        "DELETE FROM operations WHERE actor=?1 AND undone=1",
        [actor],
    )?;
    connection.execute(
        "INSERT INTO operations(actor,before_json,after_json) VALUES(?1,?2,?3)",
        params![
            actor,
            serde_json::to_string(before)?,
            serde_json::to_string(after)?
        ],
    )?;
    connection.execute("DELETE FROM operations WHERE actor=?1 AND seq NOT IN (SELECT seq FROM operations WHERE actor=?1 ORDER BY seq DESC LIMIT ?2)",params![actor,storage::HISTORY_LIMIT])?;
    Ok(())
}

pub(crate) fn snapshot_dependencies(snapshot: &Snapshot) -> Vec<EntityId> {
    if snapshot.deleted {
        return Vec::new();
    }
    match &snapshot.entity {
        Entity::Task(v) => v.list_id.map(EntityId::List).into_iter().collect(),
        Entity::Event(v) => v.calendar_id.map(EntityId::Calendar).into_iter().collect(),
        Entity::TimeBlock(v) => vec![EntityId::Task(v.task_id)],
        Entity::Reminder(v) => vec![v.target.clone()],
        _ => Vec::new(),
    }
}

pub(crate) fn validate_snapshot(snapshot: &Snapshot) -> Result<()> {
    if snapshot.revision() == 0 || snapshot.revision() > i64::MAX as u64 {
        return Err(invalid("entity revision must be 1..=i64::MAX"));
    }
    let uuid = match snapshot.id() {
        EntityId::Task(v) => v.0,
        EntityId::List(v) => v.0,
        EntityId::Event(v) => v.0,
        EntityId::TimeBlock(v) => v.0,
        EntityId::Calendar(v) => v.0,
        EntityId::Reminder(v) => v.0,
    };
    if uuid.is_nil() {
        return Err(invalid("entity ID must not be nil"));
    }
    let validate_title = |value: &str| -> Result<()> {
        if title(value.to_owned())? != value {
            return Err(invalid("stored title must be trimmed"));
        }
        Ok(())
    };
    match &snapshot.entity {
        Entity::List(v) => validate_title(&v.title)?,
        Entity::Calendar(v) => validate_title(&v.title)?,
        Entity::TimeBlock(v) => timed_range(v.start, v.end, &v.timezone)?,
        Entity::Reminder(v) => {
            if !matches!(
                v.target,
                EntityId::Task(_) | EntityId::Event(_) | EntityId::TimeBlock(_)
            ) {
                return Err(invalid(
                    "reminder target must be a task, event or time block",
                ));
            }
            match v.trigger {
                ReminderTrigger::At(at) => instant(at, chrono_tz::UTC)?,
                ReminderTrigger::Before { minutes } if minutes > 525600 => {
                    return Err(invalid("reminder offset must not exceed one year"));
                }
                _ => {}
            }
        }
        Entity::Task(v) => {
            validate_title(&v.title)?;
            due(&v.due)?;
            if v.exceptions.len() > 1000 {
                return Err(invalid(
                    "a series supports at most 1000 explicit exceptions",
                ));
            }
            match (&v.recurrence, &v.due) {
                (Some(rule), Some(due)) => {
                    let anchor = crate::recurrence::due_anchor(due)?;
                    crate::recurrence::validate(rule, anchor)?;
                    crate::recurrence::validate_exception_dates(
                        anchor,
                        rule,
                        &v.exceptions.keys().copied().collect::<Vec<_>>(),
                        |date| crate::recurrence::due_at(due, date).is_some(),
                    )?;
                    for exception in v.exceptions.values() {
                        if let TaskException::Override { title, due, .. } = exception {
                            if let Some(value) = title {
                                validate_title(value)?;
                            }
                            self::due(&Some(due.clone()))?;
                        }
                    }
                }
                (Some(_), None) => return Err(invalid("recurring tasks require a due date")),
                (None, _) if !v.exceptions.is_empty() => {
                    return Err(invalid("exceptions require a recurrence rule"));
                }
                _ => {}
            }
        }
        Entity::Event(v) => {
            validate_title(&v.title)?;
            schedule(&v.schedule)?;
            if v.exceptions.len() > 1000 {
                return Err(invalid(
                    "a series supports at most 1000 explicit exceptions",
                ));
            }
            if let Some(rule) = &v.recurrence {
                let anchor = crate::recurrence::event_anchor(&v.schedule)?;
                crate::recurrence::validate(rule, anchor)?;
                crate::recurrence::validate_exception_dates(
                    anchor,
                    rule,
                    &v.exceptions.keys().copied().collect::<Vec<_>>(),
                    |date| crate::recurrence::event_at(&v.schedule, date).is_some(),
                )?;
                for exception in v.exceptions.values() {
                    if let EventException::Override {
                        title,
                        schedule: value,
                    } = exception
                    {
                        if let Some(value) = title {
                            validate_title(value)?;
                        }
                        schedule(value)?;
                    }
                }
            } else if !v.exceptions.is_empty() {
                return Err(invalid("exceptions require a recurrence rule"));
            }
        }
    }
    Ok(())
}

fn target_reminders(connection: &Connection, targets: &[EntityId]) -> Result<Vec<Snapshot>> {
    let targets: HashSet<_> = targets.iter().collect();
    let mut statement = connection.prepare("SELECT data_json FROM reminders WHERE deleted=0")?;
    let json = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut result = Vec::new();
    for value in json {
        let reminder: Reminder = serde_json::from_str(&value)?;
        if targets.contains(&reminder.target) {
            result.push(Snapshot {
                entity: Entity::Reminder(reminder),
                deleted: false,
            });
        }
    }
    Ok(result)
}
fn deleted_with_reminders(
    connection: &Connection,
    value: Snapshot,
    id: EntityId,
) -> Result<(Vec<Snapshot>, Vec<Snapshot>, Outcome)> {
    let mut before = vec![value];
    before.extend(target_reminders(connection, std::slice::from_ref(&id))?);
    let mut after = before.clone();
    for value in &mut after {
        value.deleted = true;
        value.set_revision(next_revision(value.revision())?);
    }
    Ok((before, after, Outcome::Deleted(id)))
}
