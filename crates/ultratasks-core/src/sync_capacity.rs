//! Capacity is enforced transactionally so every accepted local database can be
//! bootstrapped/recovered using the bounded version-1 snapshot protocol.
use crate::{
    Error, Result,
    storage::{self, Snapshot},
};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const MAX_RECORDS: i64 = 100_000;
// Leave room for the versioned JSON envelope and record separators.
pub(crate) const MAX_RECORD_BYTES: i64 = 32 * 1024 * 1024 - 1024;

/// Caller owns the initialization transaction.
pub(crate) fn initialize(connection: &Connection) -> Result<()> {
    let tables: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('sync_capacity_records','sync_capacity_total')",
        [], |row| row.get(0),
    )?;
    if tables == 0 {
        connection.execute_batch(
            "CREATE TABLE sync_capacity_records (
                kind TEXT NOT NULL, id TEXT NOT NULL, bytes INTEGER NOT NULL CHECK(bytes>0),
                PRIMARY KEY(kind,id)
            );
            CREATE TABLE sync_capacity_total (
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                records INTEGER NOT NULL CHECK(records>=0), bytes INTEGER NOT NULL CHECK(bytes>=0)
            );
            INSERT INTO sync_capacity_total VALUES(1,0,0);",
        )?;
        track(connection, &storage::all_snapshots(connection)?)?;
    } else if tables != 2 {
        return Err(Error::UnrecognizedDatabase);
    }
    let total: (i64, i64) = connection.query_row(
        "SELECT records,bytes FROM sync_capacity_total WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let actual: (i64, i64) = connection.query_row(
        "SELECT count(*),COALESCE(sum(bytes),0) FROM sync_capacity_records",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if total != actual || total.0 < 0 || total.1 < 0 {
        return Err(Error::UnrecognizedDatabase);
    }
    // This is a projection of domain records, not an independent source of truth.
    // Audit once on open/restore so a damaged counter cannot bypass the bound.
    let records = storage::all_snapshots(connection)?;
    if i64::try_from(records.len()).ok() != Some(actual.0) {
        return Err(Error::UnrecognizedDatabase);
    }
    let mut statement = connection
        .prepare_cached("SELECT bytes FROM sync_capacity_records WHERE kind=?1 AND id=?2")?;
    for record in &records {
        let (kind, id) = storage::key(&record.id());
        let stored: Option<i64> = statement
            .query_row(params![kind, id], |row| row.get(0))
            .optional()?;
        let expected = i64::try_from(serde_json::to_vec(record)?.len())
            .ok()
            .and_then(|n| n.checked_add(1));
        if stored.is_none() || stored != expected {
            return Err(Error::UnrecognizedDatabase);
        }
    }
    validate_capacity(total.0, total.1)
}

/// Call in the SAME transaction as all changed records, including remote writes
/// and tombstones. A capacity failure rolls the whole operation back.
pub(crate) fn track(connection: &Connection, after: &[Snapshot]) -> Result<()> {
    let (mut count, mut bytes): (i64, i64) = connection.query_row(
        "SELECT records,bytes FROM sync_capacity_total WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    for record in after {
        let (kind, id) = storage::key(&record.id());
        let previous: Option<i64> = connection
            .query_row(
                "SELECT bytes FROM sync_capacity_records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()?;
        let size = i64::try_from(serde_json::to_vec(record)?.len())
            .map_err(|_| Error::InvalidInput("record exceeds the supported size".into()))?
            + 1;
        count += i64::from(previous.is_none());
        bytes = bytes
            .checked_sub(previous.unwrap_or(0))
            .and_then(|n| n.checked_add(size))
            .ok_or_else(|| Error::InvalidInput("dataset size overflow".into()))?;
        // Apply all deltas before checking the final size: a transaction may
        // shrink one large record while growing another.
        connection.execute(
            "INSERT INTO sync_capacity_records(kind,id,bytes) VALUES(?1,?2,?3)
             ON CONFLICT(kind,id) DO UPDATE SET bytes=excluded.bytes",
            params![kind, id, size],
        )?;
    }
    validate_capacity(count, bytes)?;
    connection.execute(
        "UPDATE sync_capacity_total SET records=?1,bytes=?2 WHERE singleton=1",
        params![count, bytes],
    )?;
    Ok(())
}

fn validate_capacity(records: i64, bytes: i64) -> Result<()> {
    if records > MAX_RECORDS || bytes > MAX_RECORD_BYTES {
        return Err(Error::InvalidInput(
            "dataset capacity exceeded: protocol v1 supports 100000 records (including deleted records) and 32 MiB of logical data; nothing was saved".into(),
        ));
    }
    Ok(())
}
