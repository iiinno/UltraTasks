use crate::{Error, Result, SecretToken};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub id: Uuid,
    pub name: String,
    pub revoked: bool,
}
#[derive(Debug)]
pub struct DeviceCredential {
    pub device: Device,
    pub token: SecretToken,
}

pub struct AuthStore {
    connection: Connection,
}
impl AuthStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id: i64 = tx.pragma_query_value(None, "application_id", |r| r.get(0))?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if id == 0 && version == 0 && count == 0 {
            tx.execute_batch("CREATE TABLE devices(id TEXT PRIMARY KEY,name TEXT NOT NULL,token_hash BLOB NOT NULL UNIQUE,revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))); PRAGMA application_id=1431586133; PRAGMA user_version=1;")?;
        } else if id != 1431586133 || version != 1 {
            return Err(Error::Configuration(
                "unrecognized credential database".into(),
            ));
        }
        tx.prepare("SELECT id,name,token_hash,revoked FROM devices LIMIT 0")?;
        tx.commit()?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self { connection })
    }

    pub fn provision(&mut self, name: &str) -> Result<DeviceCredential> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
            return Err(Error::Configuration(
                "device name must be 1..=128 characters without controls".into(),
            ));
        }
        let mut entropy = [0u8; 32];
        getrandom::fill(&mut entropy)
            .map_err(|_| Error::Configuration("operating-system randomness unavailable".into()))?;
        let token = SecretToken::parse(hex::encode(entropy))?;
        let device = Device {
            id: Uuid::now_v7(),
            name: name.to_owned(),
            revoked: false,
        };
        self.connection.execute(
            "INSERT INTO devices(id,name,token_hash) VALUES(?1,?2,?3)",
            params![
                device.id.to_string(),
                device.name,
                Sha256::digest(token.expose().as_bytes()).to_vec()
            ],
        )?;
        Ok(DeviceCredential { device, token })
    }

    pub fn revoke(&mut self, id: Uuid) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE devices SET revoked=1 WHERE id=?1 AND revoked=0",
            [id.to_string()],
        )? != 0)
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        let mut statement = self
            .connection
            .prepare("SELECT id,name,revoked FROM devices ORDER BY id")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, revoked) = row?;
            Ok(Device {
                id: id
                    .parse()
                    .map_err(|_| Error::Configuration("corrupt device ID".into()))?,
                name,
                revoked,
            })
        })
        .collect()
    }

    pub fn authenticate(&self, token: &SecretToken) -> Result<Option<Uuid>> {
        let digest = Sha256::digest(token.expose().as_bytes()).to_vec();
        let row: Option<(String, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT id,token_hash FROM devices WHERE token_hash=?1 AND revoked=0",
                [&digest],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            Some((id, stored)) if bool::from(stored.ct_eq(&digest)) => {
                Ok(Some(id.parse().map_err(|_| {
                    Error::Configuration("corrupt device ID".into())
                })?))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn random_tokens_are_revocable_and_never_stored_as_plaintext() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("auth.db");
        let mut store = AuthStore::open(&path).unwrap();
        let first = store.provision("desktop").unwrap();
        let second = store.provision("phone").unwrap();
        assert_ne!(first.token.expose(), second.token.expose());
        assert_eq!(
            store.authenticate(&first.token).unwrap(),
            Some(first.device.id)
        );
        assert!(store.revoke(first.device.id).unwrap());
        assert_eq!(store.authenticate(&first.token).unwrap(), None);
        assert_eq!(
            store.authenticate(&second.token).unwrap(),
            Some(second.device.id)
        );
        let plaintext: String = store
            .connection
            .query_row(
                "SELECT hex(token_hash) FROM devices WHERE id=?1",
                [second.device.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(plaintext.to_lowercase(), second.token.expose());
        assert!(!format!("{first:?}").contains(first.token.expose()));
    }
}
