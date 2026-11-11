use crate::{
    calendar::*,
    history::{self, Delta, Limits},
    model::*,
    storage::{self, Data, Image},
};
use rusqlite::{Connection, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use std::{fs::File, marker::PhantomData, path::Path, rc::Rc};

pub struct Store {
    pub(crate) connection: Connection,
    _lock: File,
    pub(crate) info: StoreInfo,
    pub(crate) limits: Limits,
    recovery: bool,
    #[cfg(feature = "test-support")]
    pub(crate) fixed_now: Option<Instant>,
    #[cfg(feature = "test-support")]
    pub(crate) fail: Option<crate::test_support::FailPoint>,
    thread_bound: PhantomData<Rc<()>>,
}
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    State,
    History,
    BeforeCommit,
    AfterCommit,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path, None, None)
    }
    pub(crate) fn open_inner(
        path: &Path,
        actor: Option<ActorId>,
        now: Option<Instant>,
    ) -> Result<Self> {
        let (connection, lock, mut info) = storage::open(path)?;
        if let Some(actor) = actor {
            info.actor = actor;
        }
        let mut store = Self {
            connection,
            _lock: lock,
            info,
            limits: Limits::default(),
            recovery: false,
            #[cfg(feature = "test-support")]
            fixed_now: now,
            #[cfg(feature = "test-support")]
            fail: None,
            thread_bound: PhantomData,
        };
        let initial_now = now.map_or_else(|| store.now(), Instant::unix_micros);
        store.transaction(|s| history::prune(&s.connection, initial_now, s.limits))?;
        Ok(store)
    }
    pub fn info(&self) -> &StoreInfo {
        &self.info
    }
    pub(crate) fn ready(&self) -> Result<()> {
        if self.recovery {
            Err(Error::RecoveryRequired)
        } else {
            Ok(())
        }
    }
    pub(crate) fn now(&self) -> i64 {
        #[cfg(feature = "test-support")]
        if let Some(now) = self.fixed_now {
            return now.unix_micros();
        }
        jiff::Timestamp::now().as_microsecond()
    }
    pub(crate) fn inject(&mut self, stage: Stage) -> Result<()> {
        #[cfg(feature = "test-support")]
        {
            use crate::test_support::FailPoint;
            let target = match stage {
                Stage::State => FailPoint::AfterStateWrite,
                Stage::History => FailPoint::AfterHistoryWrite,
                Stage::BeforeCommit => FailPoint::BeforeCommit,
                Stage::AfterCommit => FailPoint::AfterCommitBeforeReply,
            };
            if self.fail == Some(target) {
                self.fail = None;
                let after = matches!(stage, Stage::AfterCommit);
                if after {
                    self.recovery = true;
                }
                return Err(Error::Storage {
                    stage: if matches!(stage, Stage::BeforeCommit | Stage::AfterCommit) {
                        StorageStage::Commit
                    } else {
                        StorageStage::Write
                    },
                    kind: StorageKind::Io,
                    outcome: if after {
                        CommitKnowledge::Unknown
                    } else {
                        CommitKnowledge::NotCommitted
                    },
                });
            }
        }
        let _ = stage;
        Ok(())
    }
    fn rollback(&mut self) -> Result<()> {
        if !self.connection.is_autocommit()
            && let Err(error) = self.connection.execute_batch("ROLLBACK")
        {
            self.recovery = true;
            return Err(storage::sql_error(StorageStage::Rollback, error));
        }
        Ok(())
    }
    pub(crate) fn transaction<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.ready()?;
        self.connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| storage::sql_error(StorageStage::Begin, e))?;
        let value = match f(self) {
            Ok(value) => value,
            Err(error) => {
                self.rollback()?;
                return Err(error);
            }
        };
        if let Err(error) = self.connection.execute_batch("COMMIT") {
            if self.connection.is_autocommit() {
                self.recovery = true;
                let mut error = storage::sql_error(StorageStage::Commit, error);
                if let Error::Storage { outcome, .. } = &mut error {
                    *outcome = CommitKnowledge::Unknown;
                }
                return Err(error);
            }
            self.rollback()?;
            return Err(storage::sql_error(StorageStage::Commit, error));
        }
        Ok(value)
    }
    fn write<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<Commit<T>>) -> Result<Commit<T>> {
        let result = self.transaction(|s| {
            history::prune(&s.connection, s.now(), s.limits)?;
            let commit = f(s)?;
            if commit.operation.is_some() {
                s.inject(Stage::BeforeCommit)?;
            }
            Ok(commit)
        })?;
        if result.operation.is_some() {
            self.inject(Stage::AfterCommit)?;
        }
        Ok(result)
    }
    pub(crate) fn read<T>(&mut self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.read_with_progress_handler(
            f,
            |connection| {
                let mut remaining = storage::QUERY_VM_BUDGET / storage::PROGRESS_INTERVAL as u64;
                connection.progress_handler(
                    storage::PROGRESS_INTERVAL,
                    Some(move || {
                        remaining = remaining.saturating_sub(1);
                        remaining == 0
                    }),
                )
            },
            |connection| connection.progress_handler(0, None::<fn() -> bool>),
        )
    }
    fn read_with_progress_handler<T>(
        &mut self,
        f: impl FnOnce(&Self) -> Result<T>,
        register: impl FnOnce(&mut Connection) -> rusqlite::Result<()>,
        unregister: impl FnOnce(&mut Connection) -> rusqlite::Result<()>,
    ) -> Result<T> {
        self.ready()?;
        self.connection
            .execute_batch("BEGIN")
            .map_err(|e| storage::sql_error(StorageStage::Begin, e))?;
        let value = match register(&mut self.connection) {
            Ok(()) => f(self),
            Err(error) => Err(storage::sql_error(StorageStage::Read, error)),
        };
        // 登録が失敗してもcallbackが設定済みの場合があるため、rollbackより先に解除する。
        if let Err(error) = unregister(&mut self.connection) {
            self.recovery = true;
            self.rollback()?;
            return Err(storage::sql_error(StorageStage::Read, error));
        }
        match value {
            Ok(value) => {
                if let Err(error) = self.connection.execute_batch("COMMIT") {
                    self.rollback()?;
                    return Err(storage::sql_error(StorageStage::Read, error));
                }
                Ok(value)
            }
            Err(error) => {
                self.rollback()?;
                Err(error)
            }
        }
    }
    pub(crate) fn edited(
        &self,
        id: EntityId,
        expected: Option<Revision>,
        data: Data,
    ) -> Result<Delta> {
        let before = if let Some(revision) = expected {
            storage::expected(&self.connection, id, revision)?.image
        } else {
            if storage::load(&self.connection, id)?.is_some() {
                return Err(Error::AlreadyExists(id));
            }
            Image::absent(id)
        };
        Ok(Delta::replace(before, Some(data)))
    }
    pub(crate) fn apply(
        &mut self,
        mut changes: Vec<Delta>,
        kind: OperationKind,
        focus: EntityId,
    ) -> Result<(Option<OperationId>, Vec<Change>)> {
        if changes.iter().all(|d| d.before.data == d.after.data) {
            return Ok((None, Vec::new()));
        }
        let operation = OperationId(new_id()?);
        for delta in &mut changes {
            delta.after.position = Some(operation.0);
        }
        let bytes = history::encode(&changes, self.limits)?;
        let mut changed = Vec::with_capacity(changes.len());
        for delta in &changes {
            changed.push(storage::write_image(&self.connection, &delta.after)?);
        }
        for delta in &changes {
            storage::references(&self.connection, &delta.after, &self.info)?;
        }
        self.inject(Stage::State)?;
        history::insert(
            &self.connection,
            &self.info,
            history::NewOperation {
                id: operation,
                kind,
                focus,
                created_us: self.now(),
            },
            &changes,
            &bytes,
        )?;
        history::prune(&self.connection, self.now(), self.limits)?;
        history::bump_generation(&self.connection)?;
        self.inject(Stage::History)?;
        Ok((Some(operation), changed))
    }
    fn committed<T>(
        &self,
        value: T,
        outcome: (Option<OperationId>, Vec<Change>),
    ) -> Result<Commit<T>> {
        Ok(Commit {
            value,
            operation: outcome.0,
            changed: outcome.1,
            generation: storage::generation(&self.connection)?,
            history: history::state(&self.connection, &self.info)?,
        })
    }
    fn task(&self, id: TaskId) -> Result<Task> {
        let Entity::Task(task) =
            storage::require_live(&self.connection, EntityId::Task(id))?.entity()?
        else {
            return Err(storage::corrupt());
        };
        Ok(task)
    }
    pub fn save_task(&mut self, edit: Save<TaskId, TaskData>) -> Result<Commit<Task>> {
        self.write(|s| {
            let (id, revision, data) = match edit {
                Save::Create { id, data } => (id, None, data),
                Save::Update { expected, data } => (expected.id, Some(expected.revision), data),
            };
            let entity = EntityId::Task(id);
            let delta = s.edited(entity, revision, Data::task(data))?;
            let kind = match (&delta.before.data, &delta.after.data) {
                (None, _) => OperationKind::TaskCreated,
                (Some(Data::Task { done: a, .. }), Some(Data::Task { done: b, .. })) if a != b => {
                    if *b {
                        OperationKind::TaskCompleted
                    } else {
                        OperationKind::TaskReopened
                    }
                }
                _ => OperationKind::TaskEdited,
            };
            let outcome = s.apply(vec![delta], kind, entity)?;
            s.committed(s.task(id)?, outcome)
        })
    }
    pub fn save_event(&mut self, edit: Save<EventId, EventData>) -> Result<Commit<Event>> {
        self.write(|s| {
            let (id, revision, data) = match edit {
                Save::Create { id, data } => (id, None, data),
                Save::Update { expected, data } => (expected.id, Some(expected.revision), data),
            };
            let entity = EntityId::Event(id);
            let delta = s.edited(entity, revision, Data::event(data))?;
            let kind = if revision.is_none() {
                OperationKind::EventCreated
            } else {
                OperationKind::EventEdited
            };
            let outcome = s.apply(vec![delta], kind, entity)?;
            let Entity::Event(event) = storage::require_live(&s.connection, entity)?.entity()?
            else {
                return Err(storage::corrupt());
            };
            s.committed(event, outcome)
        })
    }
    pub fn save_time_block(
        &mut self,
        task: Expected<TaskId>,
        edit: Save<TimeBlockId, BlockData>,
    ) -> Result<Commit<BlockSaved>> {
        self.write(|s| {
            let parent = storage::expected(&s.connection, EntityId::Task(task.id), task.revision)?;
            let (id, revision, data) = match edit {
                Save::Create { id, data } => (id, None, data),
                Save::Update { expected, data } => (expected.id, Some(expected.revision), data),
            };
            let entity = EntityId::TimeBlock(id);
            let delta = s.edited(entity, revision, Data::block(task.id, data))?;
            if let Some(Data::Block { task: actual, .. }) = &delta.before.data
                && *actual != task.id.0
            {
                return Err(Error::TimeBlockTaskMismatch {
                    block: id,
                    expected: task.id,
                    actual: TaskId::from_bytes(*actual)?,
                });
            }
            let parent = Delta::replace(parent.image.clone(), parent.image.data);
            let kind = if revision.is_none() {
                OperationKind::BlockCreated
            } else {
                OperationKind::BlockEdited
            };
            let outcome = s.apply(vec![parent, delta], kind, entity)?;
            let Entity::TimeBlock(block) =
                storage::require_live(&s.connection, entity)?.entity()?
            else {
                return Err(storage::corrupt());
            };
            s.committed(
                BlockSaved {
                    task: s.task(task.id)?,
                    block,
                },
                outcome,
            )
        })
    }
    pub fn save_collection(&mut self, edit: CollectionSave) -> Result<Commit<Collection>> {
        self.write(|s| {
            let (id, revision, data) = match edit {
                CollectionSave::List(edit) => match edit {
                    Save::Create { id, data } => (
                        EntityId::List(id),
                        None,
                        Data::List(data.as_str().to_owned()),
                    ),
                    Save::Update { expected, data } => (
                        EntityId::List(expected.id),
                        Some(expected.revision),
                        Data::List(data.as_str().to_owned()),
                    ),
                },
                CollectionSave::Calendar(edit) => match edit {
                    Save::Create { id, data } => (
                        EntityId::Calendar(id),
                        None,
                        Data::Calendar(data.as_str().to_owned()),
                    ),
                    Save::Update { expected, data } => (
                        EntityId::Calendar(expected.id),
                        Some(expected.revision),
                        Data::Calendar(data.as_str().to_owned()),
                    ),
                },
            };
            let delta = s.edited(id, revision, data)?;
            let kind = if revision.is_none() {
                OperationKind::CollectionCreated
            } else {
                OperationKind::CollectionEdited
            };
            let outcome = s.apply(vec![delta], kind, id)?;
            let collection = match storage::require_live(&s.connection, id)?.entity()? {
                Entity::List(list) => Collection::List(list),
                Entity::Calendar(calendar) => Collection::Calendar(calendar),
                _ => return Err(storage::corrupt()),
            };
            s.committed(collection, outcome)
        })
    }
    pub fn delete(&mut self, target: DeleteTarget) -> Result<Commit<()>> {
        self.write(|s| {
            let (id, revision, kind) = match target {
                DeleteTarget::Task(k) => {
                    (EntityId::Task(k.id), k.revision, OperationKind::TaskDeleted)
                }
                DeleteTarget::Event(k) => (
                    EntityId::Event(k.id),
                    k.revision,
                    OperationKind::EventDeleted,
                ),
                DeleteTarget::TimeBlock { block, .. } => (
                    EntityId::TimeBlock(block.id),
                    block.revision,
                    OperationKind::BlockDeleted,
                ),
                DeleteTarget::List(k) => (
                    EntityId::List(k.id),
                    k.revision,
                    OperationKind::CollectionDeleted,
                ),
                DeleteTarget::Calendar(k) => (
                    EntityId::Calendar(k.id),
                    k.revision,
                    OperationKind::CollectionDeleted,
                ),
            };
            let before = storage::expected(&s.connection, id, revision)?.image;
            let mut deltas = Vec::new();
            if let DeleteTarget::TimeBlock { task, block } = target {
                let parent =
                    storage::expected(&s.connection, EntityId::Task(task.id), task.revision)?;
                let Some(Data::Block { task: actual, .. }) = &before.data else {
                    return Err(storage::corrupt());
                };
                if *actual != task.id.0 {
                    return Err(Error::TimeBlockTaskMismatch {
                        block: block.id,
                        expected: task.id,
                        actual: TaskId::from_bytes(*actual)?,
                    });
                }
                deltas.push(Delta::replace(parent.image.clone(), parent.image.data));
            }
            if let EntityId::Task(task) = id {
                for block in storage::task_blocks(&s.connection, task)? {
                    let image =
                        storage::require_live(&s.connection, EntityId::TimeBlock(block.key.id))?
                            .image;
                    deltas.push(Delta::replace(image, None));
                }
            }
            deltas.push(Delta::replace(before, None));
            let outcome = s.apply(deltas, kind, id)?;
            s.committed((), outcome)
        })
    }
    pub fn history(&mut self) -> Result<HistoryState> {
        self.transaction(|s| {
            history::prune(&s.connection, s.now(), s.limits)?;
            history::state(&s.connection, &s.info)
        })
    }
    pub fn undo(&mut self, target: UndoTarget) -> Result<Commit<()>> {
        self.replay(
            target.database,
            target.actor,
            target.operation,
            target.epoch,
            false,
        )
    }
    pub fn redo(&mut self, target: RedoTarget) -> Result<Commit<()>> {
        self.replay(
            target.database,
            target.actor,
            target.operation,
            target.epoch,
            true,
        )
    }
    fn replay(
        &mut self,
        database: DatabaseId,
        actor: ActorId,
        operation: OperationId,
        epoch: u64,
        redo: bool,
    ) -> Result<Commit<()>> {
        self.write(|s| {
            let deltas = history::target_changes(
                &s.connection,
                &s.info,
                database,
                actor,
                operation,
                epoch,
                redo,
            )?;
            let mut changed = Vec::with_capacity(deltas.len());
            for delta in &deltas {
                changed.push(storage::write_image(
                    &s.connection,
                    if redo { &delta.after } else { &delta.before },
                )?);
            }
            for delta in &deltas {
                let image = if redo { &delta.after } else { &delta.before };
                let entity = image.entity_id()?;
                storage::references(&s.connection, image, &s.info).map_err(
                    |error| match error {
                        Error::NotFound(_)
                        | Error::NotEmpty(_)
                        | Error::ProtectedCollection(_)
                        | Error::LimitExceeded { .. } => Error::HistoryConflict { entity },
                        other => other,
                    },
                )?;
            }
            s.inject(Stage::State)?;
            s.connection
                .execute(
                    "UPDATE history SET undone=?2 WHERE operation_id=?1",
                    params![operation.0, !redo],
                )
                .map_err(|e| storage::sql_error(StorageStage::Write, e))?;
            history::bump_generation(&s.connection)?;
            s.inject(Stage::History)?;
            s.committed((), (Some(operation), changed))
        })
    }
    pub fn get(&mut self, id: EntityId) -> Result<Entity> {
        self.read(|s| storage::require_live(&s.connection, id)?.entity())
    }
    pub fn task_detail(&mut self, id: TaskId) -> Result<TaskDetail> {
        self.read(|s| {
            Ok(TaskDetail {
                task: s.task(id)?,
                blocks: storage::task_blocks(&s.connection, id)?,
            })
        })
    }
}

struct PageEdges {
    previous: Option<Vec<u8>>,
    next: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Key {
    major: i64,
    kind: u8,
    id: [u8; 16],
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    database: [u8; 16],
    generation: u64,
    query: String,
    key: Key,
}
struct Selection {
    sql: String,
    values: Vec<Value>,
}
impl Selection {
    fn count(&self, connection: &Connection) -> Result<u64> {
        connection
            .prepare_cached(&format!("SELECT count(*) FROM ({})", self.sql))
            .and_then(|mut s| s.query_row(params_from_iter(&self.values), |r| r.get(0)))
            .map_err(|e| storage::sql_error(StorageStage::Read, e))
    }
    fn keys(
        &self,
        connection: &Connection,
        edge: Option<(&Key, &str)>,
        reverse: bool,
        limit: usize,
    ) -> Result<Vec<Key>> {
        let mut values = self.values.clone();
        let condition = if let Some((key, comparison)) = edge {
            let i = values.len();
            values.extend([
                Value::Integer(key.major),
                Value::Integer(key.kind.into()),
                Value::Blob(key.id.to_vec()),
            ]);
            format!(
                "WHERE (major,kind,id) {comparison} (?{},?{},?{})",
                i + 1,
                i + 2,
                i + 3
            )
        } else {
            String::new()
        };
        values.push(Value::Integer(limit as i64));
        let order = if reverse { "DESC" } else { "ASC" };
        let sql = format!(
            "SELECT major,kind,id FROM ({}) {condition} ORDER BY major {order},kind {order},id {order} LIMIT ?{}",
            self.sql,
            values.len()
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
        let rows = statement
            .query_map(params_from_iter(&values), |r| {
                Ok(Key {
                    major: r.get(0)?,
                    kind: r.get(1)?,
                    id: r.get(2)?,
                })
            })
            .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| storage::sql_error(StorageStage::Read, e))
    }
}
fn bind(values: &mut Vec<Value>, value: Value) -> String {
    values.push(value);
    format!("?{}", values.len())
}
fn task_selection(query: &TaskQuery) -> Result<Selection> {
    let mut values = Vec::new();
    let mut filters = vec!["t.deleted=0".to_owned()];
    if let Some(list) = query.list {
        filters.push(format!(
            "t.list_id={}",
            bind(&mut values, Value::Blob(list.0.to_vec()))
        ));
    }
    if query.status != StatusFilter::Any {
        filters.push(format!(
            "t.status={}",
            bind(
                &mut values,
                Value::Integer(i64::from(query.status == StatusFilter::Done))
            )
        ));
    }
    if let Some(search) = &query.title_contains {
        filters.push(format!(
            "instr(t.title,{})>0",
            bind(&mut values, Value::Text(search.as_str().to_owned()))
        ));
    }
    match &query.due {
        DueFilter::Any => (),
        DueFilter::Undated => filters.push("t.due_date IS NULL AND t.due_us IS NULL".to_owned()),
        DueFilter::ThroughSelectedDay => {
            let (a, b) = query.day.days().bounds();
            if a.add_days(1)? != b {
                return Err(invalid(Field::Query, InputProblem::RequiresSingleDay));
            }
            let date = bind(&mut values, Value::Text(a.iso()));
            let end = bind(
                &mut values,
                Value::Integer(query.day.instant_bounds().1.unix_micros()),
            );
            filters.push(format!("(t.due_date<={date} OR t.due_us<{end})"));
        }
        DueFilter::Within(window) => {
            let (a, b) = window.days().bounds();
            let date_a = bind(&mut values, Value::Text(a.iso()));
            let date_b = bind(&mut values, Value::Text(b.iso()));
            let (a, b) = window.instant_bounds();
            let time_a = bind(&mut values, Value::Integer(a.unix_micros()));
            let time_b = bind(&mut values, Value::Integer(b.unix_micros()));
            filters.push(format!("((t.due_date>={date_a} AND t.due_date<{date_b}) OR (t.due_us>={time_a} AND t.due_us<{time_b}))"));
        }
    }
    if query.blocks != BlockFilter::Any {
        let (a, b) = query.day.instant_bounds();
        let a = bind(&mut values, Value::Integer(a.unix_micros()));
        let b = bind(&mut values, Value::Integer(b.unix_micros()));
        let not = if query.blocks == BlockFilter::NotInSelectedWindow {
            "NOT "
        } else {
            ""
        };
        filters.push(format!("{not}EXISTS(SELECT 1 FROM blocks b WHERE b.deleted=0 AND b.task_id=t.id AND b.start_us<{b} AND b.end_us>{a} AND {a}<{b})"));
    }
    Ok(Selection {
        sql: format!(
            "SELECT t.created_seq AS major,0 AS kind,t.id FROM tasks t WHERE {}",
            filters.join(" AND ")
        ),
        values,
    })
}
impl Store {
    fn decode_cursor(
        &self,
        bytes: &[u8],
        query: &str,
        generation: DatabaseRevision,
    ) -> Result<Key> {
        let cursor: Cursor = serde_json::from_slice(bytes).map_err(|_| Error::CursorMismatch)?;
        if cursor.database != self.info.database.0 || cursor.query != query {
            return Err(Error::CursorMismatch);
        }
        if cursor.generation != generation.0 {
            return Err(Error::StalePage);
        }
        Ok(cursor.key)
    }
    fn cursor(&self, key: &Key, query: &str, generation: DatabaseRevision) -> Result<Vec<u8>> {
        serde_json::to_vec(&Cursor {
            database: self.info.database.0,
            generation: generation.0,
            query: query.to_owned(),
            key: key.clone(),
        })
        .map_err(|_| storage::corrupt())
    }
    fn cursors(
        &self,
        selection: &Selection,
        keys: &[Key],
        query: &str,
        generation: DatabaseRevision,
    ) -> Result<PageEdges> {
        let mut previous = None;
        let mut next = None;
        if let (Some(first), Some(last)) = (keys.first(), keys.last()) {
            if !selection
                .keys(&self.connection, Some((first, "<")), true, 1)?
                .is_empty()
            {
                previous = Some(self.cursor(first, query, generation)?);
            }
            if !selection
                .keys(&self.connection, Some((last, ">")), false, 1)?
                .is_empty()
            {
                next = Some(self.cursor(last, query, generation)?);
            }
        }
        Ok(PageEdges { previous, next })
    }
    fn task_row(&self, id: TaskId, window: &CalendarWindow) -> Result<TaskRow> {
        let task = self.task(id)?;
        let (a, b) = window.instant_bounds();
        let sql = "SELECT (SELECT title FROM lists WHERE id=?4 AND deleted=0),count(*),coalesce(sum(start_us<?3 AND end_us>?2 AND ?2<?3),0),coalesce(sum(CASE WHEN start_us<?3 AND end_us>?2 AND ?2<?3 THEN min(end_us,?3)-max(start_us,?2) ELSE 0 END),0),(SELECT id FROM blocks WHERE deleted=0 AND task_id=?1 AND start_us<?3 AND end_us>?2 AND ?2<?3 ORDER BY start_us,id LIMIT 1) FROM blocks WHERE deleted=0 AND task_id=?1";
        let (title, total, in_window, micros, first): (String, u16, u16, u64, Option<[u8; 16]>) =
            self.connection
                .prepare_cached(sql)
                .and_then(|mut s| {
                    s.query_row(
                        params![id.0, a.unix_micros(), b.unix_micros(), task.data.list.0],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                    )
                })
                .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
        let first_in_window = first
            .map(|id| {
                let Entity::TimeBlock(block) = storage::require_live(
                    &self.connection,
                    EntityId::TimeBlock(TimeBlockId::from_bytes(id)?),
                )?
                .entity()?
                else {
                    return Err(storage::corrupt());
                };
                Ok(block.data.span)
            })
            .transpose()?;
        Ok(TaskRow {
            task,
            list_title: Title::new(&title)?,
            blocks: BlockSummary {
                total_for_task: total,
                in_window,
                allocated_micros_in_window: micros,
                first_in_window,
            },
        })
    }
    pub fn tasks(&mut self, query: TaskQuery, page: TaskPageRequest) -> Result<TaskPage> {
        self.read(|s| {
            let selection = task_selection(&query)?;
            let generation = storage::generation(&s.connection)?;
            let fingerprint = format!("tasks:{query:?}");
            let size = usize::from(page.size.get());
            let mut keys = match &page.position {
                TaskPosition::First => selection.keys(&s.connection, None, false, size)?,
                TaskPosition::After(cursor) => selection.keys(
                    &s.connection,
                    Some((
                        &s.decode_cursor(&cursor.opaque, &fingerprint, generation)?,
                        ">",
                    )),
                    false,
                    size,
                )?,
                TaskPosition::Before(cursor) => {
                    let mut keys = selection.keys(
                        &s.connection,
                        Some((
                            &s.decode_cursor(&cursor.opaque, &fingerprint, generation)?,
                            "<",
                        )),
                        true,
                        size,
                    )?;
                    keys.reverse();
                    keys
                }
                TaskPosition::Around(id) => {
                    let row = storage::load(&s.connection, EntityId::Task(*id))?
                        .ok_or(Error::NotFound(EntityId::Task(*id)))?;
                    let anchor = Key {
                        major: row.created_seq,
                        kind: 0,
                        id: id.0,
                    };
                    let mut before =
                        selection.keys(&s.connection, Some((&anchor, "<")), true, size / 2)?;
                    before.reverse();
                    let after = selection.keys(
                        &s.connection,
                        Some((&anchor, ">=")),
                        false,
                        size - before.len(),
                    )?;
                    before.extend(after);
                    if before.len() < size {
                        let edge = before.first().unwrap_or(&anchor);
                        let mut extra = selection.keys(
                            &s.connection,
                            Some((edge, "<")),
                            true,
                            size - before.len(),
                        )?;
                        extra.reverse();
                        extra.extend(before);
                        before = extra;
                    }
                    before
                }
            };
            let total = selection.count(&s.connection)?;
            let PageEdges { previous, next } =
                s.cursors(&selection, &keys, &fingerprint, generation)?;
            let items = keys
                .drain(..)
                .map(|key| s.task_row(TaskId::from_bytes(key.id)?, &query.day))
                .collect::<Result<Vec<_>>>()?;
            Ok(Page {
                items,
                previous: previous.map(|opaque| TaskCursor { opaque }),
                next: next.map(|opaque| TaskCursor { opaque }),
                total,
                generation,
            })
        })
    }
    pub fn task_rows(&mut self, query: TaskQuery, ids: &[TaskId]) -> Result<TaskRefresh> {
        self.read(|s| {
            let mut unique = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for id in ids {
                if seen.insert(*id) {
                    if unique.len() == MAX_REFRESH_TASKS {
                        return Err(Error::LimitExceeded {
                            kind: LimitKind::RefreshIds,
                            maximum: MAX_REFRESH_TASKS as u64,
                            observed_at_least: MAX_REFRESH_TASKS as u64 + 1,
                        });
                    }
                    unique.push(*id);
                }
            }
            let selection = task_selection(&query)?;
            let total = selection.count(&s.connection)?;
            let mut rows = Vec::with_capacity(unique.len());
            let mut matching_ids = Vec::new();
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM ({}) WHERE id=?{})",
                selection.sql,
                selection.values.len() + 1
            );
            let mut statement = s
                .connection
                .prepare_cached(&sql)
                .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
            for id in unique {
                if storage::load(&s.connection, EntityId::Task(id))?
                    .is_some_and(|r| r.image.data.is_some())
                {
                    rows.push(s.task_row(id, &query.day)?);
                    let mut values = selection.values.clone();
                    values.push(Value::Blob(id.0.to_vec()));
                    let matches: bool = statement
                        .query_row(params_from_iter(values), |r| r.get(0))
                        .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
                    if matches {
                        matching_ids.push(id);
                    }
                }
            }
            Ok(TaskRefresh {
                rows,
                matching_ids,
                total,
                generation: storage::generation(&s.connection)?,
            })
        })
    }
    pub fn agenda(
        &mut self,
        query: AgendaQuery,
        page: PageRequest<AgendaCursor>,
    ) -> Result<Page<AgendaItem, AgendaCursor>> {
        self.read(|s| {
            let mut values=Vec::new();
            let calendar = query.calendar.map(|id| format!(" AND calendar_id={}",bind(&mut values,Value::Blob(id.0.to_vec())))).unwrap_or_default();
            let sql = match &query.range {
                AgendaRange::AllDay(days) => {
                    days.query_days()?;
                    let (a,b)=days.bounds();
                    let a=bind(&mut values,Value::Text(a.iso())); let b=bind(&mut values,Value::Text(b.iso()));
                    format!("SELECT CAST(replace(date_start,'-','') AS INTEGER) AS major,1 AS kind,id FROM events WHERE deleted=0 AND date_start IS NOT NULL AND date_start<{b} AND date_end>{a}{calendar}")
                }
                AgendaRange::Timed(window) => {
                    let (a,b)=window.bounds();
                    let a=bind(&mut values,Value::Integer(a.unix_micros())); let b=bind(&mut values,Value::Integer(b.unix_micros()));
                    format!("SELECT start_us AS major,1 AS kind,id FROM events WHERE deleted=0 AND start_us IS NOT NULL AND start_us<{b} AND end_us>{a} AND {a}<{b}{calendar} UNION ALL SELECT start_us AS major,2 AS kind,id FROM blocks WHERE deleted=0 AND start_us<{b} AND end_us>{a} AND {a}<{b}{calendar}")
                }
            };
            let selection=Selection { sql,values };
            let generation=storage::generation(&s.connection)?;
            let fingerprint=format!("agenda:{query:?}");
            let (edge,reverse)=match &page.position {
                PagePosition::First=>(None,false),
                PagePosition::After(c)=>(Some(s.decode_cursor(&c.opaque,&fingerprint,generation)?),false),
                PagePosition::Before(c)=>(Some(s.decode_cursor(&c.opaque,&fingerprint,generation)?),true),
            };
            let mut keys=selection.keys(&s.connection,edge.as_ref().map(|key| (key,if reverse { "<" } else { ">" })),reverse,usize::from(page.size.get()))?;
            if reverse { keys.reverse(); }
            let total=selection.count(&s.connection)?;
            let PageEdges { previous,next }=s.cursors(&selection,&keys,&fingerprint,generation)?;
            let items=keys.into_iter().map(|key| {
                match storage::require_live(&s.connection,storage::join(key.kind,key.id)?)?.entity()? {
                    Entity::Event(event)=>Ok(AgendaItem::Event(event)),
                    Entity::TimeBlock(block)=>{
                        let task=s.task(block.task)?;
                        Ok(AgendaItem::TimeBlock { block,task:TaskSummary { key:task.key,title:task.data.title,status:task.data.status } })
                    }
                    _=>Err(storage::corrupt()),
                }
            }).collect::<Result<Vec<_>>>()?;
            Ok(Page { items,previous:previous.map(|opaque| AgendaCursor { opaque }),next:next.map(|opaque| AgendaCursor { opaque }),total,generation })
        })
    }
    pub fn collections(
        &mut self,
        page: PageRequest<CollectionCursor>,
    ) -> Result<Page<Collection, CollectionCursor>> {
        self.read(|s| {
            let selection=Selection { sql:"SELECT 0 AS major,3 AS kind,id FROM lists WHERE deleted=0 UNION ALL SELECT 0 AS major,4 AS kind,id FROM calendars WHERE deleted=0".to_owned(),values:Vec::new() };
            let generation=storage::generation(&s.connection)?;
            let fingerprint="collections";
            let (edge,reverse)=match &page.position {
                PagePosition::First=>(None,false),
                PagePosition::After(c)=>(Some(s.decode_cursor(&c.opaque,fingerprint,generation)?),false),
                PagePosition::Before(c)=>(Some(s.decode_cursor(&c.opaque,fingerprint,generation)?),true),
            };
            let mut keys=selection.keys(&s.connection,edge.as_ref().map(|key| (key,if reverse { "<" } else { ">" })),reverse,usize::from(page.size.get()))?;
            if reverse { keys.reverse(); }
            let total=selection.count(&s.connection)?;
            let PageEdges { previous,next }=s.cursors(&selection,&keys,fingerprint,generation)?;
            let items=keys.into_iter().map(|key| match storage::require_live(&s.connection,storage::join(key.kind,key.id)?)?.entity()? {
                Entity::List(list)=>Ok(Collection::List(list)),Entity::Calendar(calendar)=>Ok(Collection::Calendar(calendar)),_=>Err(storage::corrupt()),
            }).collect::<Result<Vec<_>>>()?;
            Ok(Page { items,previous:previous.map(|opaque| CollectionCursor { opaque }),next:next.map(|opaque| CollectionCursor { opaque }),total,generation })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task_data(store: &Store) -> TaskData {
        TaskData {
            title: Title::new("日本語で作業").unwrap(),
            list: store.info.default_list,
            status: TaskStatus::Open,
            due: None,
        }
    }
    fn block_data(store: &Store, hour: u8) -> BlockData {
        let start = Instant::parse(&format!("2026-11-11T{hour:02}:00:00+09:00")).unwrap();
        let end = Instant::from_unix_micros(start.unix_micros() + 3_600_000_000).unwrap();
        BlockData {
            calendar: store.info.default_calendar,
            span: ZonedSpan::new(start, end, ZoneId::parse("Asia/Tokyo").unwrap()).unwrap(),
        }
    }
    #[test]
    fn vertical_two_blocks_complete_undo_redo_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vertical.sqlite");
        let mut store = Store::open(&path).unwrap();
        assert!(matches!(Store::open(&path), Err(Error::AlreadyOpen)));
        let id = TaskId::new().unwrap();
        let created = store
            .save_task(Save::Create {
                id,
                data: task_data(&store),
            })
            .unwrap()
            .value;
        let first = store
            .save_time_block(
                created.key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: block_data(&store, 10),
                },
            )
            .unwrap()
            .value;
        let second = store
            .save_time_block(
                first.task.key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: block_data(&store, 14),
                },
            )
            .unwrap()
            .value;
        let mut data = second.task.data.clone();
        data.status = TaskStatus::Done;
        let done = store
            .save_task(Save::Update {
                expected: second.task.key,
                data,
            })
            .unwrap();
        let mut revision = done.value.key.revision;
        for _ in 0..3 {
            let target = store.history().unwrap().undo.unwrap().target;
            store.undo(target).unwrap();
            let task = store.task_detail(id).unwrap().task;
            assert!(task.key.revision > revision);
            revision = task.key.revision;
        }
        assert!(store.task_detail(id).unwrap().blocks.is_empty());
        for _ in 0..3 {
            let target = store.history().unwrap().redo.unwrap().target;
            store.redo(target).unwrap();
            let task = store.task_detail(id).unwrap().task;
            assert!(task.key.revision > revision);
            revision = task.key.revision;
        }
        let detail = store.task_detail(id).unwrap();
        assert_eq!(detail.blocks.len(), 2);
        assert_eq!(detail.task.data.status, TaskStatus::Done);
        assert!(matches!(
            store.save_task(Save::Update {
                expected: created.key,
                data: task_data(&store)
            }),
            Err(Error::Conflict { .. })
        ));
        let history = store.history().unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.task_detail(id).unwrap(), detail);
        assert_eq!(store.history().unwrap(), history);
    }
    fn query(status: StatusFilter) -> TaskQuery {
        let day = Date::parse("2026-11-11").unwrap();
        TaskQuery {
            day: CalendarWindow::new(
                DateSpan::new(day, day.add_days(1).unwrap()).unwrap(),
                ZoneId::parse("Asia/Tokyo").unwrap(),
            )
            .unwrap(),
            list: None,
            status,
            due: DueFilter::Any,
            blocks: BlockFilter::Any,
            title_contains: None,
        }
    }
    #[test]
    fn query_keysets_anchor_refresh_and_clipped_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("queries.sqlite")).unwrap();
        let mut tasks = Vec::new();
        for i in 0..8 {
            let mut data = task_data(&store);
            data.title = Title::new(&format!("日本語_% {i}")).unwrap();
            tasks.push(
                store
                    .save_task(Save::Create {
                        id: TaskId::new().unwrap(),
                        data,
                    })
                    .unwrap()
                    .value,
            );
        }
        let q = query(StatusFilter::Open);
        let size = PageSize::new(3).unwrap();
        let first = store
            .tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::First,
                },
            )
            .unwrap();
        assert_eq!(first.total, 8);
        assert!(first.previous.is_none());
        let second = store
            .tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::After(first.next.clone().unwrap()),
                },
            )
            .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|r| r.task.key.id)
                .collect::<Vec<_>>(),
            tasks[3..6].iter().map(|t| t.key.id).collect::<Vec<_>>()
        );
        let back = store
            .tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::Before(second.previous.unwrap()),
                },
            )
            .unwrap();
        assert_eq!(first.items, back.items);
        let around = store
            .tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::Around(tasks[4].key.id),
                },
            )
            .unwrap();
        assert_eq!(around.items[1].task.key.id, tasks[4].key.id);
        let near_end = store
            .tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::Around(tasks[7].key.id),
                },
            )
            .unwrap();
        assert_eq!(near_end.items.len(), 3);
        assert!(near_end.next.is_none());
        let mut data = tasks[4].data.clone();
        data.status = TaskStatus::Done;
        let done = store
            .save_task(Save::Update {
                expected: tasks[4].key,
                data,
            })
            .unwrap()
            .value;
        assert!(matches!(
            store.tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::After(first.next.unwrap())
                }
            ),
            Err(Error::StalePage)
        ));
        let around = store
            .tasks(
                q.clone(),
                TaskPageRequest {
                    size,
                    position: TaskPosition::Around(done.key.id),
                },
            )
            .unwrap();
        assert_eq!(around.items[1].task.key.id, tasks[5].key.id);
        let refresh = store
            .task_rows(
                q.clone(),
                &[
                    done.key.id,
                    done.key.id,
                    tasks[0].key.id,
                    TaskId::new().unwrap(),
                ],
            )
            .unwrap();
        assert_eq!(
            (
                refresh.rows.len(),
                refresh.matching_ids.len(),
                refresh.total
            ),
            (2, 1, 7)
        );
        let too_many = (0..=MAX_REFRESH_TASKS)
            .map(|_| TaskId::new().unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(
            store.task_rows(q.clone(), &too_many),
            Err(Error::LimitExceeded {
                kind: LimitKind::RefreshIds,
                observed_at_least: 130,
                ..
            })
        ));
        assert!(
            store
                .task_rows(q.clone(), &too_many[..MAX_REFRESH_TASKS])
                .unwrap()
                .rows
                .is_empty()
        );
        let mut literal = q.clone();
        literal.title_contains = Some(SearchText::new("_%").unwrap());
        assert_eq!(
            store
                .tasks(
                    literal,
                    TaskPageRequest {
                        size,
                        position: TaskPosition::First
                    }
                )
                .unwrap()
                .total,
            7
        );
        let mut changed_query = q.clone();
        changed_query.status = StatusFilter::Any;
        assert!(matches!(
            store.tasks(
                changed_query,
                TaskPageRequest {
                    size,
                    position: TaskPosition::After(around.next.unwrap())
                }
            ),
            Err(Error::CursorMismatch)
        ));
        let span = ZonedSpan::new(
            Instant::parse("2026-11-10T23:00:00+09:00").unwrap(),
            Instant::parse("2026-11-11T01:00:00+09:00").unwrap(),
            ZoneId::parse("Asia/Tokyo").unwrap(),
        )
        .unwrap();
        let block = BlockData {
            calendar: store.info.default_calendar,
            span,
        };
        let a = store
            .save_time_block(
                tasks[0].key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: block.clone(),
                },
            )
            .unwrap()
            .value;
        store
            .save_time_block(
                a.task.key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: block,
                },
            )
            .unwrap();
        let rows = store.task_rows(q.clone(), &[tasks[0].key.id]).unwrap();
        assert_eq!(rows.rows[0].blocks.in_window, 2);
        assert_eq!(
            rows.rows[0].blocks.allocated_micros_in_window,
            7_200_000_000
        );
        let mut with_blocks = q.clone();
        with_blocks.blocks = BlockFilter::InSelectedWindow;
        assert_eq!(
            store
                .tasks(
                    with_blocks,
                    TaskPageRequest {
                        size,
                        position: TaskPosition::First
                    }
                )
                .unwrap()
                .total,
            1
        );
        let window = q
            .day
            .clip(
                Instant::parse("2026-11-11T01:00:00+09:00").unwrap(),
                Instant::parse("2026-11-11T02:00:00+09:00").unwrap(),
            )
            .unwrap();
        assert_eq!(
            store
                .agenda(
                    AgendaQuery {
                        range: AgendaRange::Timed(window),
                        calendar: None
                    },
                    PageRequest::first(size)
                )
                .unwrap()
                .total,
            0
        );
        let current = store.task_detail(tasks[5].key.id).unwrap().task;
        store.delete(DeleteTarget::Task(current.key)).unwrap();
        let around = store
            .tasks(
                q,
                TaskPageRequest {
                    size,
                    position: TaskPosition::Around(current.key.id),
                },
            )
            .unwrap();
        assert_eq!(around.items[1].task.key.id, tasks[6].key.id);
    }
    #[test]
    fn progress_registration_failure_clears_handler_and_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("register.sqlite")).unwrap();
        let result: Result<()> = store.read_with_progress_handler(
            |_| panic!("登録に失敗したreadを実行してはいけない"),
            |connection| {
                connection.progress_handler(1, Some(|| true))?;
                Err(rusqlite::Error::InvalidQuery)
            },
            |connection| connection.progress_handler(0, None::<fn() -> bool>),
        );
        assert!(matches!(
            result,
            Err(Error::Storage {
                stage: StorageStage::Read,
                kind: StorageKind::Io,
                outcome: CommitKnowledge::NotCommitted,
            })
        ));
        assert!(store.connection.is_autocommit());
        let saved = store
            .save_task(Save::Create {
                id: TaskId::new().unwrap(),
                data: task_data(&store),
            })
            .unwrap()
            .value;
        assert_eq!(
            store.get(EntityId::Task(saved.key.id)).unwrap(),
            Entity::Task(saved)
        );
    }
    #[test]
    fn progress_unregistration_failure_rolls_back_and_requires_reopen() {
        for read_fails in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("unregister.sqlite");
            let mut store = Store::open(&path).unwrap();
            let saved = store
                .save_task(Save::Create {
                    id: TaskId::new().unwrap(),
                    data: task_data(&store),
                })
                .unwrap()
                .value;
            let result = store.read_with_progress_handler(
                |_| {
                    if read_fails {
                        Err(Error::NotFound(EntityId::Task(saved.key.id)))
                    } else {
                        Ok(())
                    }
                },
                |connection| connection.progress_handler(1, Some(|| false)),
                |_| Err(rusqlite::Error::InvalidQuery),
            );
            assert!(matches!(
                result,
                Err(Error::Storage {
                    stage: StorageStage::Read,
                    kind: StorageKind::Io,
                    outcome: CommitKnowledge::NotCommitted,
                })
            ));
            assert!(store.connection.is_autocommit());
            assert!(matches!(
                store.get(EntityId::Task(saved.key.id)),
                Err(Error::RecoveryRequired)
            ));
            assert!(matches!(
                store.delete(DeleteTarget::Task(saved.key)),
                Err(Error::RecoveryRequired)
            ));
            drop(store);
            assert_eq!(
                Store::open(&path)
                    .unwrap()
                    .get(EntityId::Task(saved.key.id))
                    .unwrap(),
                Entity::Task(saved)
            );
        }
    }
    #[test]
    fn query_budget_failure_leaves_store_usable() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.sqlite")).unwrap();
        let result:Result<u64>=store.read(|s| s.connection.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<2000000) SELECT sum(x) FROM n",[],|r|r.get(0)).map_err(|e|storage::sql_error(StorageStage::Read,e)));
        assert!(matches!(result, Err(Error::QueryBudgetExceeded)));
        assert!(store.connection.is_autocommit());
        assert!(
            store
                .collections(PageRequest::first(PageSize::new(10).unwrap()))
                .is_ok()
        );
    }
    #[test]
    fn collections_events_and_references_share_atomic_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("relations.sqlite")).unwrap();
        let Collection::Calendar(calendar) = store
            .save_collection(CollectionSave::Calendar(Save::Create {
                id: CalendarId::new().unwrap(),
                data: Title::new("予定用").unwrap(),
            }))
            .unwrap()
            .value
        else {
            panic!()
        };
        let mut data = EventData {
            title: Title::new("終日の予定").unwrap(),
            calendar: calendar.key.id,
            when: EventWhen::AllDay(query(StatusFilter::Any).day.days()),
        };
        let created = store
            .save_event(Save::Create {
                id: EventId::new().unwrap(),
                data: data.clone(),
            })
            .unwrap();
        assert!(matches!(
            store.delete(DeleteTarget::Calendar(calendar.key)),
            Err(Error::NotEmpty(_))
        ));
        data.when = EventWhen::Timed(block_data(&store, 10).span);
        let event = store
            .save_event(Save::Update {
                expected: created.value.key,
                data,
            })
            .unwrap()
            .value;
        let page = store
            .agenda(
                AgendaQuery {
                    range: AgendaRange::Timed(query(StatusFilter::Any).day.timed()),
                    calendar: Some(calendar.key.id),
                },
                PageRequest::first(PageSize::new(1).unwrap()),
            )
            .unwrap();
        assert_eq!(page.total, 1);
        store.delete(DeleteTarget::Event(event.key)).unwrap();
        let deleted = store.delete(DeleteTarget::Calendar(calendar.key)).unwrap();
        store.undo(deleted.history.undo.unwrap().target).unwrap();
        let target = store.history().unwrap().undo.unwrap().target;
        store.undo(target).unwrap();
        assert!(matches!(
            store.get(EntityId::Event(event.key.id)),
            Ok(Entity::Event(_))
        ));
        let default = store
            .get(EntityId::Calendar(store.info.default_calendar))
            .unwrap();
        let Entity::Calendar(default) = default else {
            panic!()
        };
        assert!(matches!(
            store.delete(DeleteTarget::Calendar(default.key)),
            Err(Error::ProtectedCollection(_))
        ));
        let page = store
            .collections(PageRequest::first(PageSize::new(1).unwrap()))
            .unwrap();
        let next = store
            .collections(PageRequest {
                size: PageSize::new(1).unwrap(),
                position: PagePosition::After(page.next.unwrap()),
            })
            .unwrap();
        let previous = store
            .collections(PageRequest {
                size: PageSize::new(1).unwrap(),
                position: PagePosition::Before(next.previous.unwrap()),
            })
            .unwrap();
        assert_eq!(page.items, previous.items);
    }
}
