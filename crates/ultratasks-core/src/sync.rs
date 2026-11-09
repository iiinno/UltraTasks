//! Durable synchronization state and protocol. Network I/O belongs to the host.
//! Server versions and local edit revisions are intentionally independent.
use crate::{
    Engine, EntityId, Error, Result, engine,
    storage::{self, Snapshot},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_RECORDS: usize = 100_000;
pub const MAX_WIRE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SyncMutation {
    pub expected_version: u64,
    #[serde(default)]
    pub guard_only: bool,
    pub record: Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SyncPush {
    pub protocol: u32,
    pub generation: Uuid,
    pub operation_id: Uuid,
    pub mutations: Vec<SyncMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SyncPushResult {
    Accepted {
        generation: Uuid,
        records: Vec<Snapshot>,
    },
    Conflict {
        generation: Uuid,
        current: Vec<Option<Snapshot>>,
        related: Vec<Snapshot>,
    },
    ResetRequired {
        generation: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncBatch {
    pub cursor: u64,
    pub records: Vec<Snapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncPull {
    pub protocol: u32,
    pub generation: Uuid,
    pub cursor: u64,
    pub has_more: bool,
    pub batches: Vec<SyncBatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncServerSnapshot {
    pub protocol: u32,
    pub generation: Uuid,
    pub cursor: u64,
    pub records: Vec<Snapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPosition {
    pub generation: Option<Uuid>,
    pub cursor: u64,
    pub pending_operations: u64,
    pub has_conflict: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncResolution {
    KeepLocal,
    KeepRemote,
}

#[derive(Debug, Clone)]
pub struct SyncResetGuard {
    records: Vec<Snapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub request: SyncPush,
    /// Same order as request.mutations. None means the server never saw the ID.
    pub remote: Vec<Option<Snapshot>>,
    pub related: Vec<Snapshot>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidInput(message.into())
}
fn json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}
fn decode<T: serde::de::DeserializeOwned>(value: String) -> Result<T> {
    Ok(serde_json::from_str(&value)?)
}
fn uuid(value: String) -> Result<Uuid> {
    value.parse().map_err(|_| invalid("invalid sync UUID"))
}
fn sql_u64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid("sync cursor exceeds SQLite integer range"))
}
fn read_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(column)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
fn key(id: &EntityId) -> String {
    let (kind, id) = storage::key(id);
    format!("{kind}:{id}")
}

pub(crate) fn initialize(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='sync_local_state' AND type='table')",
        [],
        |r| r.get(0),
    )?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS sync_local_state (
       singleton INTEGER PRIMARY KEY CHECK(singleton=1), generation TEXT, cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0));
       INSERT OR IGNORE INTO sync_local_state(singleton) VALUES(1);
       CREATE TABLE IF NOT EXISTS sync_baselines (entity_key TEXT PRIMARY KEY, record_json TEXT NOT NULL);
       CREATE TABLE IF NOT EXISTS sync_outbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, records_json TEXT NOT NULL, before_json TEXT NOT NULL DEFAULT '[]');
       CREATE TABLE IF NOT EXISTS sync_prepared (singleton INTEGER PRIMARY KEY CHECK(singleton=1), upper_seq INTEGER NOT NULL, request_json TEXT NOT NULL);
       CREATE TABLE IF NOT EXISTS sync_conflicts (seq INTEGER PRIMARY KEY AUTOINCREMENT, upper_seq INTEGER NOT NULL, conflict_json TEXT NOT NULL, resolved TEXT);
       CREATE TABLE IF NOT EXISTS sync_recovery_archive (seq INTEGER PRIMARY KEY AUTOINCREMENT, local_json TEXT NOT NULL, remote_json TEXT NOT NULL, resolution TEXT NOT NULL);
       CREATE TABLE IF NOT EXISTS sync_server_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1), generation TEXT NOT NULL);
       CREATE TABLE IF NOT EXISTS sync_server_log (seq INTEGER PRIMARY KEY AUTOINCREMENT, records_json TEXT NOT NULL);
       CREATE TABLE IF NOT EXISTS sync_server_receipts (operation_id TEXT PRIMARY KEY, request_json TEXT NOT NULL, response_json TEXT NOT NULL);")?;
    crate::sync_capacity::initialize(&tx)?;
    let before_column: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sync_outbox') WHERE name='before_json')",
        [],
        |r| r.get(0),
    )?;
    if !before_column {
        tx.execute_batch(
            "ALTER TABLE sync_outbox ADD COLUMN before_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    if !exists {
        let records = storage::all_snapshots(&tx)?;
        if !records.is_empty() {
            record_local_change(&tx, &[], &records)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Must be called inside the same write transaction as the corresponding edit.
pub(crate) fn record_local_change(
    connection: &Connection,
    before: &[Snapshot],
    after: &[Snapshot],
) -> Result<()> {
    if !after.is_empty() {
        validate_records(after)?;
        crate::sync_capacity::track(connection, after)?;
        let authoritative: bool =
            connection.query_row("SELECT EXISTS(SELECT 1 FROM sync_server_state)", [], |r| {
                r.get(0)
            })?;
        if authoritative {
            connection.execute(
                "INSERT INTO sync_server_log(records_json) VALUES(?1)",
                [json(&after)?],
            )?;
        } else {
            connection.execute(
                "INSERT INTO sync_outbox(records_json,before_json) VALUES(?1,?2)",
                params![json(&after)?, json(&before)?],
            )?;
        }
    }
    Ok(())
}

fn snapshot_at_sequence(
    connection: &Connection,
    id: &EntityId,
    upper_seq: i64,
) -> Result<Snapshot> {
    let mut statement =
        connection.prepare("SELECT before_json FROM sync_outbox WHERE seq>?1 ORDER BY seq")?;
    let rows = statement.query_map([upper_seq], |r| r.get::<_, String>(0))?;
    for row in rows {
        for record in decode::<Vec<Snapshot>>(row?)? {
            if record.id() == *id && !record.deleted {
                return Ok(record);
            }
        }
    }
    storage::active(connection, id)
}
fn baseline(connection: &Connection, id: &EntityId) -> Result<Option<Snapshot>> {
    connection
        .query_row(
            "SELECT record_json FROM sync_baselines WHERE entity_key=?1",
            [key(id)],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(decode)
        .transpose()
}
fn put_baseline(connection: &Connection, record: &Snapshot) -> Result<()> {
    connection.execute("INSERT INTO sync_baselines(entity_key,record_json) VALUES(?1,?2) ON CONFLICT(entity_key) DO UPDATE SET record_json=excluded.record_json", params![key(&record.id()), json(record)?])?;
    Ok(())
}
fn generation(connection: &Connection) -> Result<Option<Uuid>> {
    connection
        .query_row(
            "SELECT generation FROM sync_local_state WHERE singleton=1",
            [],
            |r| r.get::<_, Option<String>>(0),
        )?
        .map(uuid)
        .transpose()
}
fn server_generation(connection: &Connection) -> Result<Uuid> {
    uuid(connection.query_row(
        "SELECT generation FROM sync_server_state WHERE singleton=1",
        [],
        |r| r.get(0),
    )?)
}
fn validate_records(records: &[Snapshot]) -> Result<()> {
    if records.len() > MAX_RECORDS {
        return Err(invalid("sync batch exceeds 100,000 changed records"));
    }
    let mut ids = HashSet::new();
    for record in records {
        if !ids.insert(record.id()) {
            return Err(invalid("duplicate entity in sync batch"));
        }
        engine::validate_snapshot(record)?;
    }
    if json(&records)?.len() > MAX_WIRE_BYTES {
        return Err(invalid("sync batch exceeds 64 MiB"));
    }
    Ok(())
}
fn bumped(mut record: Snapshot, current: Option<&Snapshot>) -> Result<Snapshot> {
    let revision = current
        .map_or(0, Snapshot::revision)
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| invalid("sync revision exhausted"))?;
    record.set_revision(revision);
    Ok(record)
}
fn server_conflict(
    connection: &Connection,
    generation: Uuid,
    request: &SyncPush,
    current: Vec<Option<Snapshot>>,
) -> Result<SyncPushResult> {
    let ids: HashSet<_> = request.mutations.iter().map(|v| v.record.id()).collect();
    let related = storage::all_snapshots(connection)?
        .into_iter()
        .filter(|v| !ids.contains(&v.id()))
        .collect();
    Ok(SyncPushResult::Conflict {
        generation,
        current,
        related,
    })
}
fn remote_replacement(connection: &Connection, remote: &Snapshot) -> Result<Option<Snapshot>> {
    let current = storage::get(connection, &remote.id())?;
    if let Some(known) = baseline(connection, &remote.id())?
        && known.revision() == remote.revision()
        && known.same_value(remote)
        && current
            .as_ref()
            .is_some_and(|value| value.same_value(remote))
    {
        return Ok(None);
    }
    Ok(Some(bumped(remote.clone(), current.as_ref())?))
}
fn prepared(connection: &Connection) -> Result<Option<(i64, SyncPush)>> {
    let row: Option<(i64, String)> = connection
        .query_row(
            "SELECT upper_seq,request_json FROM sync_prepared WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(seq, text)| Ok((seq, decode(text)?))).transpose()
}

impl Engine {
    pub fn sync_prepared_request(&self) -> Result<Option<SyncPush>> {
        Ok(prepared(&self.connection)?.map(|(_, request)| request))
    }

    pub fn sync_position(&self) -> Result<SyncPosition> {
        let (cursor,pending,conflict): (u64,u64,bool) = self.connection.query_row("SELECT cursor,(SELECT count(*) FROM sync_outbox),EXISTS(SELECT 1 FROM sync_conflicts WHERE resolved IS NULL) FROM sync_local_state WHERE singleton=1", [], |r| Ok((read_u64(r,0)?,read_u64(r,1)?,r.get(2)?)))?;
        Ok(SyncPosition {
            generation: generation(&self.connection)?,
            cursor,
            pending_operations: pending,
            has_conflict: conflict,
        })
    }

    /// A different generation is never silently accepted: it may be a restored server.
    pub fn sync_attach(&mut self, remote_generation: Uuid) -> Result<bool> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let authoritative: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_server_state)", [], |r| {
                r.get(0)
            })?;
        if authoritative {
            return Err(invalid(
                "a server database cannot also synchronize as a client; export into a separate local database",
            ));
        }
        match generation(&tx)? {
            Some(current) if current != remote_generation => return Ok(false),
            None => {
                tx.execute(
                    "UPDATE sync_local_state SET generation=?1 WHERE singleton=1",
                    [remote_generation.to_string()],
                )?;
            }
            _ => {}
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn sync_prepare_push(&mut self) -> Result<Option<SyncPush>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let has_conflict: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_conflicts WHERE resolved IS NULL)",
            [],
            |r| r.get(0),
        )?;
        if has_conflict {
            return Err(invalid("resolve the durable sync conflict before retrying"));
        }
        if let Some((_, request)) = prepared(&tx)? {
            return Ok(Some(request));
        }
        let remote_generation =
            generation(&tx)?.ok_or_else(|| invalid("attach a sync server first"))?;
        let mut records: HashMap<EntityId, Snapshot> = HashMap::new();
        let mut upper_seq = 0;
        let mut payload_bytes = 0usize;
        {
            let mut statement =
                tx.prepare("SELECT seq,records_json FROM sync_outbox ORDER BY seq")?;
            let rows =
                statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (seq, text) = row?;
                let group = decode::<Vec<Snapshot>>(text)?;
                let additional = group
                    .iter()
                    .filter(|v| !records.contains_key(&v.id()))
                    .count();
                // Never split one local transaction. Stop before the next group.
                if records.len() + additional > MAX_RECORDS && upper_seq > 0 {
                    break;
                }
                let mut candidate_bytes = payload_bytes;
                for record in &group {
                    if let Some(previous) = records.get(&record.id()) {
                        candidate_bytes -= json(previous)?.len();
                    }
                    candidate_bytes += json(record)?.len();
                }
                if candidate_bytes > MAX_WIRE_BYTES * 3 / 4 && upper_seq > 0 {
                    break;
                }
                for record in group {
                    records.insert(record.id(), record);
                }
                payload_bytes = candidate_bytes;
                upper_seq = seq;
                if records.len() > MAX_RECORDS {
                    return Err(invalid("one pending operation exceeds 100,000 entities"));
                }
            }
        }
        if upper_seq == 0 {
            return Ok(None);
        }
        // Include references as optimistic preconditions. A parent removed remotely
        // becomes an explicit conflict instead of resurrecting implicitly.
        let changed_ids: HashSet<_> = records.keys().cloned().collect();
        let mut visit: Vec<Snapshot> = records.values().cloned().collect();
        while let Some(record) = visit.pop() {
            for dependency in engine::snapshot_dependencies(&record) {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    records.entry(dependency.clone())
                {
                    let parent = snapshot_at_sequence(&tx, &dependency, upper_seq)?;
                    entry.insert(parent.clone());
                    visit.push(parent);
                }
            }
            if records.len() > MAX_RECORDS {
                return Err(invalid("sync dependency set exceeds 100,000 entities"));
            }
        }
        let mut records: Vec<_> = records.into_values().collect();
        records.sort_by_key(|v| key(&v.id()));
        validate_records(&records)?;
        let mutations = records
            .into_iter()
            .map(|record| {
                let expected_version = baseline(&tx, &record.id())?.map_or(0, |v| v.revision());
                let guard_only = !changed_ids.contains(&record.id());
                Ok(SyncMutation {
                    expected_version,
                    guard_only,
                    record,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let request = SyncPush {
            protocol: PROTOCOL_VERSION,
            generation: remote_generation,
            operation_id: Uuid::now_v7(),
            mutations,
        };
        tx.execute(
            "INSERT INTO sync_prepared(singleton,upper_seq,request_json) VALUES(1,?1,?2)",
            params![upper_seq, json(&request)?],
        )?;
        tx.commit()?;
        Ok(Some(request))
    }

    pub fn sync_accept_push_result(
        &mut self,
        request: &SyncPush,
        result: &SyncPushResult,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let (upper_seq, stored) =
            prepared(&tx)?.ok_or_else(|| invalid("there is no prepared sync request"))?;
        if stored != *request {
            return Err(invalid("sync response does not match the prepared request"));
        }
        match result {
            SyncPushResult::Accepted {
                generation: remote_generation,
                records,
            } => {
                if *remote_generation != request.generation
                    || records.len() != request.mutations.len()
                {
                    return Err(invalid("invalid sync acknowledgement"));
                }
                validate_records(records)?;
                for (record, sent) in records.iter().zip(&request.mutations) {
                    if record.id() != sent.record.id()
                        || !record.same_value(&sent.record)
                        || (if sent.guard_only {
                            record.revision() < sent.expected_version
                        } else {
                            record.revision() <= sent.expected_version
                        })
                    {
                        return Err(invalid("acknowledged records differ from the sent change"));
                    }
                    put_baseline(&tx, record)?;
                }
                tx.execute("DELETE FROM sync_outbox WHERE seq<=?1", [upper_seq])?;
                tx.execute("DELETE FROM sync_prepared", [])?;
            }
            SyncPushResult::Conflict {
                generation: remote_generation,
                current,
                related,
            } => {
                if *remote_generation != request.generation
                    || current.len() != request.mutations.len()
                {
                    return Err(invalid("invalid sync conflict response"));
                }
                for (remote, sent) in current.iter().zip(&request.mutations) {
                    if let Some(record) = remote {
                        engine::validate_snapshot(record)?;
                        if record.id() != sent.record.id() {
                            return Err(invalid("conflict entity mismatch"));
                        }
                    }
                }
                validate_records(related)?;
                let conflict = SyncConflict {
                    request: request.clone(),
                    remote: current.clone(),
                    related: related.clone(),
                };
                tx.execute("INSERT INTO sync_conflicts(upper_seq,conflict_json) SELECT ?1,?2 WHERE NOT EXISTS(SELECT 1 FROM sync_conflicts WHERE resolved IS NULL)", params![upper_seq,json(&conflict)?])?;
            }
            SyncPushResult::ResetRequired { .. } => {
                return Err(invalid(
                    "server generation changed; reconcile before retrying",
                ));
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn sync_conflict(&self) -> Result<Option<SyncConflict>> {
        self.connection.query_row("SELECT conflict_json FROM sync_conflicts WHERE resolved IS NULL ORDER BY seq LIMIT 1", [], |r| r.get::<_,String>(0)).optional()?.map(decode).transpose()
    }

    /// Both alternatives remain in sync_conflicts after resolution for audit/recovery.
    /// New edits invalidate the captured decision, just like an optimistic editor save.
    pub fn sync_resolve_conflict(&mut self, resolution: SyncResolution) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let row: Option<(i64,i64,String)> = tx.query_row("SELECT seq,upper_seq,conflict_json FROM sync_conflicts WHERE resolved IS NULL ORDER BY seq LIMIT 1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (conflict_seq, upper_seq, text) =
            row.ok_or_else(|| invalid("there is no sync conflict"))?;
        let conflict: SyncConflict = decode(text)?;
        let latest: i64 =
            tx.query_row("SELECT COALESCE(MAX(seq),0) FROM sync_outbox", [], |r| {
                r.get(0)
            })?;
        if latest != upper_seq {
            return Err(invalid(
                "local edits arrived after the conflict; refresh the conflict before deciding",
            ));
        }
        match resolution {
            SyncResolution::KeepLocal => {
                let mut request = conflict.request;
                request.operation_id = Uuid::now_v7();
                for (mutation, remote) in request.mutations.iter_mut().zip(&conflict.remote) {
                    mutation.expected_version = remote.as_ref().map_or(0, Snapshot::revision);
                    if remote
                        .as_ref()
                        .is_none_or(|r| !r.same_value(&mutation.record))
                    {
                        mutation.guard_only = false;
                    }
                    if let Some(record) = remote {
                        put_baseline(&tx, record)?;
                    } else {
                        tx.execute(
                            "DELETE FROM sync_baselines WHERE entity_key=?1",
                            [key(&mutation.record.id())],
                        )?;
                    }
                }
                tx.execute(
                    "UPDATE sync_prepared SET request_json=?1 WHERE singleton=1",
                    [json(&request)?],
                )?;
            }
            SyncResolution::KeepRemote => {
                let mut replacements = Vec::new();
                for remote in &conflict.related {
                    if let Some(replacement) = remote_replacement(&tx, remote)? {
                        replacements.push(replacement);
                    }
                    put_baseline(&tx, remote)?;
                }
                for (mutation, remote) in conflict.request.mutations.iter().zip(&conflict.remote) {
                    if let Some(record) = remote {
                        if let Some(replacement) = remote_replacement(&tx, record)? {
                            replacements.push(replacement);
                        }
                        put_baseline(&tx, record)?;
                    } else {
                        let current = storage::get(&tx, &mutation.record.id())?;
                        let mut target = mutation.record.clone();
                        target.deleted = true;
                        replacements.push(bumped(target, current.as_ref())?);
                        tx.execute(
                            "DELETE FROM sync_baselines WHERE entity_key=?1",
                            [key(&mutation.record.id())],
                        )?;
                    }
                }
                validate_records(&replacements)?;
                engine::check_relations(&tx, &replacements, false)?;
                crate::sync_capacity::track(&tx, &replacements)?;
                for record in &replacements {
                    storage::put(&tx, record)?;
                }
                tx.execute("DELETE FROM sync_outbox WHERE seq<=?1", [upper_seq])?;
                tx.execute("DELETE FROM sync_prepared", [])?;
            }
        }
        tx.execute(
            "UPDATE sync_conflicts SET resolved=?1 WHERE seq=?2",
            params![json(&resolution)?, conflict_seq],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Re-capture newer local edits after an unresolved conflict without discarding any.
    pub fn sync_refresh_conflict(&mut self) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        tx.execute("UPDATE sync_conflicts SET resolved='superseded-by-new-local-edits' WHERE resolved IS NULL", [])?;
        tx.execute("DELETE FROM sync_prepared", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn sync_apply_pull(&mut self, response: &SyncPull) -> Result<()> {
        if response.protocol != PROTOCOL_VERSION {
            return Err(invalid("unsupported sync protocol"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        if generation(&tx)? != Some(response.generation) {
            return Err(invalid("server generation changed"));
        }
        let pending: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox)", [], |r| r.get(0))?;
        if pending {
            return Err(invalid(
                "push or resolve local changes before applying remote changes",
            ));
        }
        let mut cursor: u64 = tx.query_row(
            "SELECT cursor FROM sync_local_state WHERE singleton=1",
            [],
            |r| read_u64(r, 0),
        )?;
        for batch in &response.batches {
            if batch.cursor <= cursor {
                return Err(invalid("non-monotonic sync cursor"));
            }
            validate_records(&batch.records)?;
            let mut replacements = Vec::new();
            for remote in &batch.records {
                // A pushed acknowledgement can be newer than this log entry.
                if let Some(known) = baseline(&tx, &remote.id())?
                    && known.revision() >= remote.revision()
                {
                    continue;
                }
                let current = storage::get(&tx, &remote.id())?;
                // A foreign write advances the local revision even when values
                // compare equal; otherwise stale edits/Undo could cross an ABA write.
                replacements.push(bumped(remote.clone(), current.as_ref())?);
                put_baseline(&tx, remote)?;
            }
            engine::check_relations(&tx, &replacements, false)?;
            crate::sync_capacity::track(&tx, &replacements)?;
            for record in &replacements {
                storage::put(&tx, record)?;
            }
            cursor = batch.cursor;
        }
        if response.cursor != cursor {
            return Err(invalid("response cursor does not match committed batches"));
        }
        tx.execute(
            "UPDATE sync_local_state SET cursor=?1 WHERE singleton=1",
            [sql_u64(cursor)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// After a routine server restart, compare every previously seen server record.
    /// Unchanged data safely gets a fresh generation; rollback/divergence needs a decision.
    pub fn sync_recover_unchanged(&mut self, remote: &SyncServerSnapshot) -> Result<bool> {
        if remote.protocol != PROTOCOL_VERSION {
            return Err(invalid("unsupported sync protocol"));
        }
        validate_records(&remote.records)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let remote_by_id: HashMap<_, _> = remote.records.iter().map(|v| (v.id(), v)).collect();
        let mut statement = tx.prepare("SELECT record_json FROM sync_baselines")?;
        let baselines = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for text in &baselines {
            let record: Snapshot = decode(text.clone())?;
            if remote_by_id
                .get(&record.id())
                .is_none_or(|v| !v.same_value(&record))
            {
                return Ok(false);
            }
        }
        // No acknowledgement may be forgotten while a different incarnation is used.
        // Its original immutable request is preserved in the recovery audit archive.
        if let Some((_, request)) = prepared(&tx)? {
            tx.execute("INSERT INTO sync_recovery_archive(local_json,remote_json,resolution) VALUES(?1,?2,'unchanged-restart')",params![json(&request)?,json(remote)?])?;
        }
        for text in baselines {
            let record: Snapshot = decode(text)?;
            put_baseline(&tx, remote_by_id[&record.id()])?;
        }
        tx.execute(
            "UPDATE sync_local_state SET generation=?1,cursor=0 WHERE singleton=1",
            [remote.generation.to_string()],
        )?;
        tx.execute("DELETE FROM sync_prepared", [])?;
        tx.execute("UPDATE sync_conflicts SET resolved='superseded-by-server-restart' WHERE resolved IS NULL",[])?;
        tx.commit()?;
        Ok(true)
    }

    /// Explicit recovery after a server restore. Both complete sides are archived.
    /// KeepLocal submits all local records with current remote versions; it remains
    /// conditional, so edits arriving after the decision become another conflict.
    pub fn sync_reset_guard(&mut self) -> Result<SyncResetGuard> {
        let tx = self.connection.transaction()?;
        let records = storage::all_snapshots(&tx)?;
        tx.commit()?;
        Ok(SyncResetGuard { records })
    }

    /// Immediate serialized recovery only. Async/UI callers must capture a reset
    /// guard before confirmation/network I/O and use sync_resolve_reset_guarded.
    pub fn sync_resolve_reset(
        &mut self,
        remote: &SyncServerSnapshot,
        resolution: SyncResolution,
    ) -> Result<()> {
        let guard = self.sync_reset_guard()?;
        self.sync_resolve_reset_guarded(remote, resolution, &guard)
    }

    /// Capture the guard before displaying confirmation or awaiting the remote snapshot.
    pub fn sync_resolve_reset_guarded(
        &mut self,
        remote: &SyncServerSnapshot,
        resolution: SyncResolution,
        guard: &SyncResetGuard,
    ) -> Result<()> {
        if remote.protocol != PROTOCOL_VERSION {
            return Err(invalid("unsupported sync protocol"));
        }
        validate_records(&remote.records)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let local = storage::all_snapshots(&tx)?;
        if local != guard.records {
            return Err(invalid(
                "local data changed after recovery confirmation; review the new state before retrying",
            ));
        }
        validate_records(&local)?;
        tx.execute(
            "INSERT INTO sync_recovery_archive(local_json,remote_json,resolution) VALUES(?1,?2,?3)",
            params![json(&local)?, json(remote)?, json(&resolution)?],
        )?;
        tx.execute("DELETE FROM sync_baselines", [])?;
        for record in &remote.records {
            put_baseline(&tx, record)?;
        }
        tx.execute("DELETE FROM sync_outbox", [])?;
        tx.execute("DELETE FROM sync_prepared", [])?;
        tx.execute("UPDATE sync_conflicts SET resolved='superseded-by-explicit-reset' WHERE resolved IS NULL",[])?;
        tx.execute(
            "UPDATE sync_local_state SET generation=?1,cursor=?2 WHERE singleton=1",
            params![remote.generation.to_string(), sql_u64(remote.cursor)?],
        )?;
        match resolution {
            SyncResolution::KeepLocal => {
                // Remote-only records are retained. Their references are validated
                // with local changes together on the server before acceptance.
                let local_ids: HashSet<_> = local.iter().map(Snapshot::id).collect();
                let mut remote_only = Vec::new();
                for record in &remote.records {
                    if !local_ids.contains(&record.id()) {
                        remote_only.push(bumped(record.clone(), None)?);
                    }
                }
                engine::check_relations(&tx, &remote_only, false)?;
                crate::sync_capacity::track(&tx, &remote_only)?;
                for record in &remote_only {
                    storage::put(&tx, record)?;
                }
                record_local_change(&tx, &[], &local)?;
            }
            SyncResolution::KeepRemote => {
                let mut desired: HashMap<_, _> =
                    remote.records.iter().map(|v| (v.id(), v.clone())).collect();
                for mut record in local.iter().cloned() {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        desired.entry(record.id())
                    {
                        record.deleted = true;
                        entry.insert(record);
                    }
                }
                let desired = desired
                    .into_values()
                    .map(|record| {
                        let previous = storage::get(&tx, &record.id())?;
                        bumped(record, previous.as_ref())
                    })
                    .collect::<Result<Vec<_>>>()?;
                engine::check_relations(&tx, &desired, false)?;
                crate::sync_capacity::track(&tx, &desired)?;
                for record in &desired {
                    storage::put(&tx, record)?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Called once by the server process, before accepting connections. Rotating
    /// on startup also detects an operator replacing the DB with an old backup.
    pub fn sync_server_start(&mut self) -> Result<Uuid> {
        let generation = Uuid::now_v7();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let initialized: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_server_state)", [], |r| {
                r.get(0)
            })?;
        if !initialized {
            let records = storage::all_snapshots(&tx)?;
            validate_records(&records)?;
            if !records.is_empty() {
                tx.execute(
                    "INSERT INTO sync_server_log(records_json) VALUES(?1)",
                    [json(&records)?],
                )?;
            }
            tx.execute("DELETE FROM sync_outbox", [])?;
            tx.execute("DELETE FROM sync_prepared", [])?;
        }
        tx.execute("INSERT INTO sync_server_state(singleton,generation) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET generation=excluded.generation",[generation.to_string()])?;
        tx.commit()?;
        Ok(generation)
    }

    pub fn sync_server_generation(&self) -> Result<Uuid> {
        server_generation(&self.connection)
    }

    pub fn sync_server_snapshot(&mut self) -> Result<SyncServerSnapshot> {
        let tx = self.connection.transaction()?;
        let records = storage::all_snapshots(&tx)?;
        validate_records(&records)?;
        let snapshot = SyncServerSnapshot {
            protocol: PROTOCOL_VERSION,
            generation: server_generation(&tx)?,
            cursor: tx.query_row(
                "SELECT COALESCE(MAX(seq),0) FROM sync_server_log",
                [],
                |r| read_u64(r, 0),
            )?,
            records,
        };
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn sync_server_push(&mut self, request: &SyncPush) -> Result<SyncPushResult> {
        if request.protocol != PROTOCOL_VERSION {
            return Err(invalid("unsupported sync protocol"));
        }
        if request.mutations.is_empty() {
            return Err(invalid("empty sync mutation"));
        }
        let records: Vec<_> = request.mutations.iter().map(|v| v.record.clone()).collect();
        validate_records(&records)?;
        let request_json = json(request)?;
        if request_json.len() > MAX_WIRE_BYTES {
            return Err(invalid("request exceeds 64 MiB"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let receipt: Option<(String, String)> = tx
            .query_row(
                "SELECT request_json,response_json FROM sync_server_receipts WHERE operation_id=?1",
                [request.operation_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((stored, response)) = receipt {
            if stored != request_json {
                return Err(invalid("operation ID was reused with a different payload"));
            }
            // Return the original response even across restart. The client records
            // its acknowledgement, then reconciles the new incarnation separately.
            return decode(response);
        }
        let generation = server_generation(&tx)?;
        if generation != request.generation {
            return Ok(SyncPushResult::ResetRequired { generation });
        }
        let current = request
            .mutations
            .iter()
            .map(|v| storage::get(&tx, &v.record.id()))
            .collect::<Result<Vec<_>>>()?;
        if request
            .mutations
            .iter()
            .zip(&current)
            .any(|(mutation, current)| {
                mutation.expected_version != current.as_ref().map_or(0, Snapshot::revision)
                    || (mutation.guard_only
                        && current
                            .as_ref()
                            .is_none_or(|v| !v.same_value(&mutation.record)))
            })
        {
            return server_conflict(&tx, generation, request, current);
        }
        let records = request
            .mutations
            .iter()
            .zip(&current)
            .map(|(mutation, current)| {
                if mutation.guard_only {
                    current
                        .clone()
                        .ok_or_else(|| invalid("missing sync dependency"))
                } else {
                    bumped(mutation.record.clone(), current.as_ref())
                }
            })
            .collect::<Result<Vec<_>>>()?;
        if engine::check_relations(&tx, &records, false).is_err() {
            return server_conflict(&tx, generation, request, current);
        }
        let changed: Vec<_> = records
            .iter()
            .zip(&request.mutations)
            .filter(|(_, mutation)| !mutation.guard_only)
            .map(|(record, _)| record.clone())
            .collect();
        crate::sync_capacity::track(&tx, &changed)?;
        for record in &changed {
            storage::put(&tx, record)?;
        }
        if !changed.is_empty() {
            tx.execute(
                "INSERT INTO sync_server_log(records_json) VALUES(?1)",
                [json(&changed)?],
            )?;
        }
        let result = SyncPushResult::Accepted {
            generation,
            records,
        };
        tx.execute("INSERT INTO sync_server_receipts(operation_id,request_json,response_json) VALUES(?1,?2,?3)",params![request.operation_id.to_string(),request_json,json(&result)?])?;
        tx.commit()?;
        Ok(result)
    }

    pub fn sync_server_pull(
        &mut self,
        expected_generation: Uuid,
        cursor: u64,
        limit: u32,
    ) -> Result<SyncPull> {
        if !(1..=100).contains(&limit) {
            return Err(invalid("sync page limit must be 1..=100 batches"));
        }
        let tx = self.connection.transaction()?;
        let generation = server_generation(&tx)?;
        if generation != expected_generation {
            return Err(invalid("server generation changed"));
        }
        let latest: u64 = tx.query_row(
            "SELECT COALESCE(MAX(seq),0) FROM sync_server_log",
            [],
            |r| read_u64(r, 0),
        )?;
        if cursor > latest {
            return Err(invalid(
                "sync cursor is ahead of server; reconciliation required",
            ));
        }
        let mut statement = tx.prepare(
            "SELECT seq,records_json FROM sync_server_log WHERE seq>?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = statement.query_map(params![sql_u64(cursor)?, limit], |r| {
            Ok((read_u64(r, 0)?, r.get::<_, String>(1)?))
        })?;
        let mut batches = Vec::new();
        let mut bytes = 0;
        for row in rows {
            let (cursor, text) = row?;
            if bytes + text.len() > MAX_WIRE_BYTES / 2 && !batches.is_empty() {
                break;
            }
            bytes += text.len();
            batches.push(SyncBatch {
                cursor,
                records: decode(text)?,
            });
        }
        drop(statement);
        let cursor = batches.last().map_or(cursor, |v| v.cursor);
        let response = SyncPull {
            protocol: PROTOCOL_VERSION,
            generation,
            cursor,
            has_more: cursor < latest,
            batches,
        };
        tx.commit()?;
        Ok(response)
    }
}
