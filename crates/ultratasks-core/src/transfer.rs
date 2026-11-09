//! Lossless logical transfer and online SQLite backups. None of these APIs replace
//! an existing database file. Use a new destination, validate it, then switch hosts.
use crate::{
    Engine, EntityId, engine,
    storage::{self, Snapshot},
};
use rusqlite::{
    Connection, OpenFlags, TransactionBehavior,
    backup::{Backup, StepResult},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_TRANSFER_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TRANSFER_RECORDS: usize = 100_000;

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    Core(#[from] crate::Error),
    #[error("database transfer failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("file transfer failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid transfer JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid transfer: {0}")]
    Invalid(String),
    #[error("destination already exists: {0}")]
    Exists(PathBuf),
    #[error("backup could not complete within 60 seconds")]
    Timeout,
}
pub type TransferResult<T> = std::result::Result<T, TransferError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// Requires a database with no records, including tombstones.
    Empty,
    /// Adds new IDs. An identical ID/value/revision is a no-op; all other
    /// collisions fail atomically. This never resurrects a deleted record.
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub inserted: usize,
    pub unchanged: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    records: Vec<Snapshot>,
}

impl Engine {
    /// Versioned logical export: records and tombstones, including recurrence
    /// exceptions. Local history, credentials, notification receipts and sync
    /// transport state belong to SQLite backups, not this portable format.
    pub fn export_json(&self) -> TransferResult<String> {
        let tx = self.connection.unchecked_transaction()?;
        storage::validate_consistency(&tx)?;
        let records = storage::all_snapshots(&tx)?;
        ensure_record_count(records.len())?;
        let value = serde_json::to_string(&Envelope {
            format: "ultratasks".into(),
            version: 1,
            records,
        })?;
        ensure_input_size(value.len())?;
        tx.commit()?;
        Ok(value)
    }

    pub fn import_json(&mut self, input: &str, mode: ImportMode) -> TransferResult<ImportReport> {
        ensure_input_size(input.len())?;
        let envelope: Envelope = serde_json::from_str(input)?;
        if envelope.format != "ultratasks" || envelope.version != 1 {
            return Err(invalid("unsupported format or version"));
        }
        // Snapshot types allow defaulted fields for schema migrations. A portable
        // format must not silently discard unknown fields supplied by a newer app.
        let raw: serde_json::Value = serde_json::from_str(input)?;
        if raw["records"] != serde_json::to_value(&envelope.records)? {
            return Err(invalid(
                "record fields must exactly match transfer format version 1",
            ));
        }
        self.import_records(envelope.records, mode)
    }

    pub(crate) fn import_records(
        &mut self,
        records: Vec<Snapshot>,
        mode: ImportMode,
    ) -> TransferResult<ImportReport> {
        ensure_record_count(records.len())?;
        let mut ids = HashSet::<EntityId>::with_capacity(records.len());
        for record in &records {
            if !ids.insert(record.id()) {
                return Err(invalid("duplicate entity identifier in input"));
            }
            engine::validate_snapshot(record)?;
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if mode == ImportMode::Empty && !storage::all_snapshots(&tx)?.is_empty() {
            return Err(invalid(
                "empty import requires a database without records or tombstones",
            ));
        }
        let mut after = Vec::new();
        let mut unchanged = 0;
        for record in records {
            if let Some(existing) = storage::get(&tx, &record.id())? {
                if existing != record {
                    return Err(invalid(format!("entity ID collision: {:?}", record.id())));
                }
                unchanged += 1;
            } else {
                after.push(record);
            }
        }
        if after.is_empty() {
            tx.commit()?;
            return Ok(ImportReport {
                inserted: 0,
                unchanged,
            });
        }
        engine::check_relations(&tx, &after, false)?;
        // Record order in a portable file has no semantic significance.
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        for record in &after {
            storage::put(&tx, record)?;
        }
        let before: Vec<_> = after
            .iter()
            .map(|record| {
                let mut absent = record.clone();
                absent.deleted = true;
                absent.set_revision(0);
                absent
            })
            .collect();
        engine::write_history(&tx, &self.actor, &before, &after)?;
        tx.commit()?;
        Ok(ImportReport {
            inserted: after.len(),
            unchanged,
        })
    }

    /// Uses SQLite's online backup API; committed WAL pages are included. The
    /// final path appears only after integrity validation and is never replaced.
    pub fn backup_to(&self, destination: impl AsRef<Path>) -> TransferResult<()> {
        let destination = destination.as_ref();
        let temporary = TemporaryDatabase::new(destination)?;
        copy_database(&self.connection, &temporary.path)?;
        validate_copy(&temporary.path)?;
        temporary.publish(destination)
    }

    /// Restores to a NEW database. The source is opened read-only, copied with
    /// SQLite, then ownership/schema/integrity/domain relations are checked on
    /// the copy. The source and any existing destination are never modified.
    pub fn restore_backup(
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> TransferResult<()> {
        let destination = destination.as_ref();
        let temporary = TemporaryDatabase::new(destination)?;
        let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let application_id: i64 =
            source.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = source.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if application_id != 0x5554_4b53 || version < 1 {
            return Err(crate::Error::UnrecognizedDatabase.into());
        }
        copy_database(&source, &temporary.path)?;
        drop(source);
        validate_copy(&temporary.path)?;
        temporary.publish(destination)
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> TransferError {
    TransferError::Invalid(message.into())
}
pub(crate) fn ensure_input_size(size: usize) -> TransferResult<()> {
    if size > MAX_TRANSFER_BYTES {
        return Err(invalid("input exceeds 32 MiB"));
    }
    Ok(())
}
fn ensure_record_count(count: usize) -> TransferResult<()> {
    if count > MAX_TRANSFER_RECORDS {
        return Err(invalid("record count exceeds 100000"));
    }
    Ok(())
}

fn copy_database(source: &Connection, path: &Path) -> TransferResult<()> {
    let mut destination = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    destination.pragma_update(None, "synchronous", "FULL")?;
    let backup = Backup::new(source, &mut destination)?;
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(60) {
            return Err(TransferError::Timeout);
        }
        match backup.step(256)? {
            StepResult::Done => break,
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(10)),
            _ => return Err(invalid("unsupported SQLite backup state")),
        }
    }
    Ok(())
}

fn validate_copy(path: &Path) -> TransferResult<()> {
    // Engine::open performs application marker/schema checks and supported
    // migrations on this private copy, never on the supplied source.
    let engine = Engine::open(path)?;
    let check: String = engine
        .connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(invalid("SQLite integrity check failed"));
    }
    if engine
        .connection
        .prepare("PRAGMA foreign_key_check")?
        .exists([])?
    {
        return Err(invalid("SQLite foreign key check failed"));
    }
    storage::validate_consistency(&engine.connection)?;
    // Publish a self-contained file without transient WAL/SHM siblings.
    engine
        .connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
    drop(engine);
    Ok(())
}

struct TemporaryDatabase {
    path: PathBuf,
}
impl TemporaryDatabase {
    fn new(destination: &Path) -> TransferResult<Self> {
        if destination.try_exists()? || fs::symlink_metadata(destination).is_ok() {
            return Err(TransferError::Exists(destination.to_owned()));
        }
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let path = parent.join(format!(".ultratasks-{}.sqlite3.tmp", uuid::Uuid::now_v7()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path)?;
        Ok(Self { path })
    }
    fn publish(self, destination: &Path) -> TransferResult<()> {
        File::open(&self.path)?.sync_all()?;
        // Same-directory hard link is an atomic no-clobber publication. rename
        // would overwrite a destination created after the initial exists check.
        fs::hard_link(&self.path, destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                TransferError::Exists(destination.to_owned())
            } else {
                TransferError::Io(error)
            }
        })?;
        #[cfg(unix)]
        {
            let parent = destination
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}
impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut path = self.path.as_os_str().to_owned();
            path.push(suffix);
            let _ = fs::remove_file(PathBuf::from(path));
        }
    }
}
