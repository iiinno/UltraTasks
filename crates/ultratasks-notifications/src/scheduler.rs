use crate::{DesiredNotification, Error, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

/// Every operation must be idempotent by `os_id`. A failed effect is retried on
/// the next reconciliation. No app-level API can promise exactly-once sound.
pub trait NotificationBackend {
    fn pending_ids(&mut self) -> Result<BTreeSet<String>>;
    fn schedule(&mut self, os_id: &str, notification: &DesiredNotification) -> Result<()>;
    fn show(&mut self, os_id: &str, notification: &DesiredNotification) -> Result<()>;
    /// Cancel a pending timer. Do not erase already delivered user-visible history.
    fn cancel(&mut self, os_id: &str) -> Result<()>;
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub scheduled: usize,
    pub shown: usize,
    pub cancelled: usize,
    pub elapsed: usize,
}
use serde::{Deserialize, Serialize};

pub struct Scheduler {
    connection: Connection,
}

impl Scheduler {
    /// Use an OS-local application-data path, not a synchronized database. A
    /// fresh device must get a fresh state file and register its own OS queue.
    pub fn open(path: impl AsRef<Path>, device_id: &str) -> Result<Self> {
        if device_id.is_empty() || device_id.len() > 128 || device_id.chars().any(char::is_control)
        {
            return Err(Error::Invalid("device ID must be 1..128 bytes".into()));
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let app: i64 = tx.pragma_query_value(None, "application_id", |r| r.get(0))?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let tables: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if app == 0 && version == 0 && tables == 0 {
            tx.execute_batch("CREATE TABLE device (id TEXT NOT NULL PRIMARY KEY); CREATE TABLE deliveries (id TEXT PRIMARY KEY, os_id TEXT NOT NULL UNIQUE, payload TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('scheduled','shown','elapsed'))); PRAGMA application_id=1431588436; PRAGMA user_version=1;")?;
            tx.execute("INSERT INTO device(id) VALUES(?1)", [device_id])?;
        } else if app != 1431588436 || version != 1 {
            return Err(Error::UnrecognizedDatabase);
        }
        let stored: String = tx.query_row("SELECT id FROM device", [], |r| r.get(0))?;
        if stored != device_id {
            return Err(Error::WrongDevice);
        }
        tx.commit()?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self { connection })
    }

    /// Reconcile on startup, after writes/sync, and periodically. The immediate
    /// transaction serializes multiple hosts. OS effects precede commit; retries
    /// replace the same OS ID if an interruption occurred between effect/commit.
    pub fn reconcile(
        &mut self,
        desired: &[DesiredNotification],
        now: DateTime<Utc>,
        backend: &mut impl NotificationBackend,
    ) -> Result<ReconcileReport> {
        if desired.len() > 4000 {
            return Err(Error::Invalid(
                "at most 4000 pending notification instances".into(),
            ));
        }
        let mut incoming = BTreeMap::new();
        let mut os_ids = BTreeSet::new();
        for value in desired {
            validate(value)?;
            let id = os_id(&value.id);
            if incoming
                .insert(value.id.clone(), (id.clone(), value))
                .is_some()
                || !os_ids.insert(id)
            {
                return Err(Error::Invalid(
                    "duplicate notification ID or OS ID collision".into(),
                ));
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut query = tx.prepare("SELECT id,os_id,payload,state FROM deliveries")?;
        let existing = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(query);
        for (old_id, old_os_id, _, _) in &existing {
            if incoming
                .iter()
                .any(|(id, (os_id, _))| id != old_id && os_id == old_os_id)
            {
                return Err(Error::Invalid(
                    "OS ID collision with existing reminder".into(),
                ));
            }
        }
        let pending = backend.pending_ids()?;
        let mut report = ReconcileReport::default();
        for (id, os_id, _, _) in &existing {
            if !incoming.contains_key(id) {
                backend.cancel(os_id)?;
                tx.execute("DELETE FROM deliveries WHERE id=?1", [id])?;
                report.cancelled += 1;
            }
        }
        // Also clean OS entries left after a state-file reset. This notifier is
        // dedicated to UltraTasks, so no other app's notifications are affected.
        for id in pending.difference(&os_ids) {
            if !existing.iter().any(|(_, v, _, _)| v == id) {
                backend.cancel(id)?;
                report.cancelled += 1;
            }
        }
        for (id, (os_id, value)) in incoming {
            let payload = serde_json::to_string(value)?;
            let previous: Option<(String, String)> = tx
                .query_row(
                    "SELECT payload,state FROM deliveries WHERE id=?1",
                    [&id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let same = previous.as_ref().is_some_and(|(old, _)| old == &payload);
            let same_instant = previous
                .as_ref()
                .map(|(old, _)| serde_json::from_str::<DesiredNotification>(old))
                .transpose()?
                .is_some_and(|old| old.fire_at == value.fire_at);
            let prior_state = previous.as_ref().map(|(_, state)| state.as_str());
            let state = if value.fire_at > now {
                if !same || !pending.contains(&os_id) {
                    backend.schedule(&os_id, value)?;
                    report.scheduled += 1;
                }
                "scheduled"
            } else if same_instant && matches!(prior_state, Some("scheduled" | "shown" | "elapsed"))
            {
                // The OS owned this due timer. Never replay it on app resume;
                // disappearance from the queue is not proof it wasn't delivered.
                report.elapsed += 1;
                "elapsed"
            } else {
                backend.show(&os_id, value)?;
                report.shown += 1;
                "shown"
            };
            tx.execute("INSERT INTO deliveries(id,os_id,payload,state) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET os_id=excluded.os_id,payload=excluded.payload,state=excluded.state",params![id,os_id,payload,state])?;
        }
        tx.commit()?;
        Ok(report)
    }
}

fn validate(value: &DesiredNotification) -> Result<()> {
    use chrono::Datelike;
    if value.id.is_empty()
        || value.id.len() > 200
        || value.id.chars().any(char::is_control)
        || value.title.is_empty()
        || value.title.chars().count() > 1000
        || value.body.chars().count() > 4000
        || value
            .title
            .chars()
            .chain(value.body.chars())
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        || !(1601..=9999).contains(&value.fire_at.year())
        || value.fire_at.timestamp_subsec_nanos() >= 1_000_000_000
    {
        return Err(Error::Invalid(
            "notification has invalid identity, content or time".into(),
        ));
    }
    Ok(())
}

fn os_id(value: &str) -> String {
    let hash = Sha256::digest(value.as_bytes());
    hash[..8].iter().map(|b| format!("{b:02x}")).collect()
}
