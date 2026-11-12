use crate::{
    calendar::DAY_US,
    model::*,
    storage::{self, Image},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub operations: u16,
    pub history_bytes: u64,
    pub operation_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            operations: MAX_HISTORY_OPERATIONS,
            history_bytes: MAX_HISTORY_BYTES,
            operation_bytes: MAX_OPERATION_BYTES,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Delta {
    pub before: Image,
    pub after: Image,
}
impl Delta {
    pub(crate) fn replace(before: Image, data: Option<storage::Data>) -> Self {
        let mut after = before.clone();
        after.data = data;
        Self { before, after }
    }
}
#[derive(Serialize, Deserialize)]
struct Payload {
    version: u16,
    changes: Vec<Delta>,
}
pub(crate) fn encode(changes: &[Delta], limits: Limits) -> Result<Vec<u8>> {
    if changes.is_empty() || changes.len() > MAX_REFRESH_TASKS {
        return Err(storage::corrupt());
    }
    let bytes = serde_json::to_vec(&Payload {
        version: 1,
        changes: changes.to_vec(),
    })
    .map_err(|_| storage::corrupt())?;
    let maximum = limits.operation_bytes.min(limits.history_bytes);
    if bytes.len() as u64 > maximum {
        return Err(Error::LimitExceeded {
            kind: LimitKind::ChangeBytes,
            maximum,
            observed_at_least: bytes.len() as u64,
        });
    }
    Ok(bytes)
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Delta>> {
    if bytes.len() as u64 > MAX_OPERATION_BYTES {
        return Err(storage::corrupt());
    }
    let payload: Payload = serde_json::from_slice(bytes).map_err(|_| storage::corrupt())?;
    if payload.version != 1
        || payload.changes.is_empty()
        || payload.changes.len() > MAX_REFRESH_TASKS
    {
        return Err(storage::corrupt());
    }
    let mut ids = std::collections::HashSet::new();
    for delta in &payload.changes {
        delta.before.validate().map_err(|_| storage::corrupt())?;
        delta.after.validate().map_err(|_| storage::corrupt())?;
        let id = delta.before.entity_id()?;
        if id != delta.after.entity_id()? || !ids.insert(id) {
            return Err(storage::corrupt());
        }
    }
    Ok(payload.changes)
}
pub(crate) fn prune(connection: &Connection, now: i64, limits: Limits) -> Result<()> {
    let cutoff = now.saturating_sub(i64::from(HISTORY_DAYS) * DAY_US);
    let mut removed = prune_selected(
        connection,
        "SELECT seq,actor_id,undone FROM history WHERE created_us <= ?1",
        [cutoff],
    )?;
    removed += prune_selected(
        connection,
        "SELECT seq,actor_id,undone FROM history ORDER BY seq DESC LIMIT -1 OFFSET ?1",
        [limits.operations],
    )?;
    loop {
        let bytes: u64 = connection
            .query_row(
                "SELECT coalesce(sum(image_bytes),0) FROM history",
                [],
                |r| r.get(0),
            )
            .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
        if bytes <= limits.history_bytes {
            break;
        }
        removed += prune_selected(
            connection,
            "SELECT seq,actor_id,undone FROM history ORDER BY seq LIMIT 1",
            [],
        )?;
    }
    if removed != 0 {
        bump_epoch(connection)?;
    }
    Ok(())
}
fn prune_selected(
    connection: &Connection,
    selection: &str,
    parameters: impl rusqlite::Params,
) -> Result<usize> {
    // Undo済み操作を失うと、そのactorの後続Redoも前提を失う。
    connection
        .execute(
            &format!(
                "WITH expired AS ({selection})
                 DELETE FROM history WHERE seq IN (
                     SELECT seq FROM expired
                     UNION
                     SELECT redo.seq FROM history AS redo
                     JOIN expired ON redo.actor_id = expired.actor_id AND redo.seq > expired.seq
                     WHERE redo.undone = 1 AND expired.undone = 1
                 )"
            ),
            parameters,
        )
        .map_err(|e| storage::sql_error(StorageStage::Write, e))
}
pub(crate) fn bump_epoch(connection: &Connection) -> Result<()> {
    connection
        .execute("UPDATE meta SET epoch=epoch+1 WHERE singleton=1", [])
        .map_err(|e| storage::sql_error(StorageStage::Write, e))?;
    Ok(())
}
pub(crate) fn bump_generation(connection: &Connection) -> Result<()> {
    connection
        .execute(
            "UPDATE meta SET generation=generation+1,epoch=epoch+1 WHERE singleton=1",
            [],
        )
        .map_err(|e| storage::sql_error(StorageStage::Write, e))?;
    Ok(())
}
pub(crate) struct NewOperation {
    pub id: OperationId,
    pub kind: OperationKind,
    pub focus: EntityId,
    pub created_us: i64,
}
pub(crate) fn insert(
    connection: &Connection,
    info: &StoreInfo,
    operation: NewOperation,
    changes: &[Delta],
    bytes: &[u8],
) -> Result<()> {
    connection
        .execute(
            "DELETE FROM history WHERE actor_id=?1 AND undone=1",
            [info.actor.0],
        )
        .map_err(|e| storage::sql_error(StorageStage::Write, e))?;
    let (focus_kind, focus_id) = storage::split(operation.focus);
    connection.execute("INSERT INTO history(operation_id,actor_id,created_us,kind,focus_kind,focus_id,affected,undone,images,image_bytes) VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9)",
        params![operation.id.0, info.actor.0, operation.created_us, operation.kind as u8, focus_kind, focus_id, changes.len() as u16, bytes, bytes.len() as u64]).map_err(|e| storage::sql_error(StorageStage::Write,e))?;
    Ok(())
}
fn kind(value: u8) -> Result<OperationKind> {
    use OperationKind::*;
    Ok(match value {
        0 => TaskCreated,
        1 => TaskEdited,
        2 => TaskCompleted,
        3 => TaskReopened,
        4 => TaskDeleted,
        5 => EventCreated,
        6 => EventEdited,
        7 => EventDeleted,
        8 => BlockCreated,
        9 => BlockEdited,
        10 => BlockDeleted,
        11 => CollectionCreated,
        12 => CollectionEdited,
        13 => CollectionDeleted,
        _ => return Err(storage::corrupt()),
    })
}
struct Head {
    operation: OperationId,
    kind: OperationKind,
    focus: EntityId,
    affected: u16,
}
fn head(connection: &Connection, actor: ActorId, redo: bool) -> Result<Option<Head>> {
    let order = if redo { "ASC" } else { "DESC" };
    let row = connection.prepare_cached(&format!("SELECT operation_id,kind,focus_kind,focus_id,affected FROM history WHERE actor_id=?1 AND undone=?2 ORDER BY seq {order} LIMIT 1"))
        .and_then(|mut s| s.query_row(params![actor.0, redo], |r| Ok((r.get::<_,[u8;16]>(0)?, r.get::<_,u8>(1)?, r.get::<_,u8>(2)?, r.get::<_,[u8;16]>(3)?, r.get::<_,u16>(4)?))).optional()).map_err(|e| storage::sql_error(StorageStage::Read,e))?;
    row.map(|(operation, k, f, id, affected)| {
        Ok(Head {
            operation: OperationId(operation),
            kind: kind(k)?,
            focus: storage::join(f, id)?,
            affected,
        })
    })
    .transpose()
}
pub(crate) fn state(connection: &Connection, info: &StoreInfo) -> Result<HistoryState> {
    let epoch = storage::epoch(connection)?;
    let undo = head(connection, info.actor, false)?.map(|h| HistoryEntry {
        target: UndoTarget {
            database: info.database,
            actor: info.actor,
            operation: h.operation,
            epoch,
        },
        kind: h.kind,
        focus: h.focus,
        affected: h.affected,
    });
    let redo = head(connection, info.actor, true)?.map(|h| HistoryEntry {
        target: RedoTarget {
            database: info.database,
            actor: info.actor,
            operation: h.operation,
            epoch,
        },
        kind: h.kind,
        focus: h.focus,
        affected: h.affected,
    });
    Ok(HistoryState { undo, redo })
}
pub(crate) fn target_changes(
    connection: &Connection,
    info: &StoreInfo,
    database: DatabaseId,
    actor: ActorId,
    operation: OperationId,
    epoch: u64,
    redo: bool,
) -> Result<Vec<Delta>> {
    if database != info.database {
        return Err(Error::History(HistoryProblem::WrongDatabase));
    }
    if actor != info.actor {
        return Err(Error::History(HistoryProblem::NotOwn));
    }
    let saved = connection
        .query_row(
            "SELECT actor_id,images FROM history WHERE operation_id=?1",
            [operation.0],
            |r| Ok((r.get::<_, [u8; 16]>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(|e| storage::sql_error(StorageStage::Read, e))?;
    let Some((saved_actor, bytes)) = saved else {
        return Err(Error::History(HistoryProblem::Expired));
    };
    if saved_actor != actor.0 {
        return Err(Error::History(HistoryProblem::NotOwn));
    }
    if epoch != storage::epoch(connection)? {
        return Err(Error::History(HistoryProblem::StaleTarget));
    }
    let Some(current) = head(connection, actor, redo)? else {
        return Err(Error::History(HistoryProblem::Empty));
    };
    if current.operation != operation {
        return Err(Error::History(HistoryProblem::NotCurrent));
    }
    let changes = decode(&bytes)?;
    for delta in &changes {
        let expected = if redo { &delta.before } else { &delta.after };
        let id = expected.entity_id()?;
        let actual = storage::load(connection, id)?
            .map(|row| row.image)
            .unwrap_or_else(|| Image::absent(id));
        if actual != *expected {
            return Err(Error::HistoryConflict { entity: id });
        }
    }
    Ok(changes)
}
