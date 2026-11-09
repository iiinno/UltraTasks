//! Bounded system operations shared by both JNI access modes. The database path
//! is a trusted host argument, never data read from an Android intent.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use ultratasks_core::{Command, Engine, ImportMode};
use ultratasks_notifications::{DevicePolicy, plan_notifications};

const MAX_REQUEST_BYTES: usize = ultratasks_core::MAX_TRANSFER_BYTES + 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
type BridgeResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    ReadWrite,
    ReadOnly,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Execute {
        request_id: String,
        commands: Vec<Command>,
    },
    Snapshot,
    History,
    Undo,
    Redo,
    Export,
    Import {
        data: Value,
        #[serde(default)]
        require_empty: bool,
    },
    ExportIcs,
    ImportIcs {
        data: String,
        #[serde(default)]
        require_empty: bool,
    },
    Backup {
        filename: String,
    },
    Restore {
        filename: String,
        destination: String,
    },
    Network {
        request: super::network::NetworkRequest,
    },
    Plan {
        timezone: String,
        enabled: bool,
        #[serde(default)]
        missed_grace_minutes: Option<u16>,
    },
}

impl Request {
    fn read_only(&self) -> bool {
        matches!(
            self,
            Self::Snapshot | Self::History | Self::Export | Self::ExportIcs | Self::Plan { .. }
        )
    }
}

struct CachedEngine {
    path: PathBuf,
    access: Access,
    engine: Engine,
}
static ENGINE: OnceLock<Mutex<Option<CachedEngine>>> = OnceLock::new();

/// One process-local database owner serializes requests and reuses the connection.
/// A host changing database or access mode replaces the cached connection. The
/// read-only entry uses SQLite READ_ONLY/query_only and never creates a database.
fn with_engine<T>(
    path: &Path,
    access: Access,
    operation: impl FnOnce(&mut Engine) -> BridgeResult<T>,
) -> BridgeResult<T> {
    let mut cache = ENGINE.get_or_init(|| Mutex::new(None)).lock().map_err(
        |_| "database owner is unavailable after an interrupted request; restart the host",
    )?;
    if !cache
        .as_ref()
        .is_some_and(|cached| cached.path == path && cached.access == access)
    {
        let engine = match access {
            Access::ReadWrite => Engine::open_for_actor(path, "android")?,
            Access::ReadOnly => Engine::open_read_only(path)?,
        };
        *cache = Some(CachedEngine {
            path: fs::canonicalize(path)?,
            access,
            engine,
        });
    }
    operation(
        &mut cache
            .as_mut()
            .ok_or("database owner did not initialize")?
            .engine,
    )
}

pub(crate) fn dispatch(path: &str, input: &str, access: Access) -> String {
    match dispatch_value(path, input, access).and_then(|value| {
        let encoded = serde_json::to_string(&value)?;
        if encoded.len() > MAX_RESPONSE_BYTES {
            return Err("response exceeds 64 MiB; narrow the request".into());
        }
        Ok(encoded)
    }) {
        Ok(encoded) => encoded,
        Err(error) => json!({"ok":false,"error":error.to_string()}).to_string(),
    }
}

fn dispatch_value(path: &str, input: &str, access: Access) -> BridgeResult<Value> {
    if input.len() > MAX_REQUEST_BYTES {
        return Err("request exceeds 32 MiB plus 64 KiB envelope".into());
    }
    let request: Request = serde_json::from_str(input)?;
    if access == Access::ReadOnly && !request.read_only() {
        return Err("operation is unavailable on a read-only connection".into());
    }
    let path = database_path(path)?;
    if let Request::Network { request } = request {
        // The network host uses its own guarded connection so waiting on a
        // provider or sync server never holds the local owner mutex.
        return super::network::dispatch(&path, request);
    }
    if let Request::Restore {
        filename,
        destination,
    } = request
    {
        let source = confined_file(&path, &filename)?;
        let destination_path = confined_file(&path, &destination)?;
        Engine::restore_backup(source, destination_path)?;
        return Ok(json!({"ok":true,"filename":destination}));
    }
    with_engine(&path, access, |engine| match request {
        Request::Execute {
            request_id,
            commands,
        } => Ok(json!({"ok":true,"outcomes":engine.execute_idempotent(&request_id,commands)?})),
        Request::Snapshot => Ok(json!({"ok":true,"records":engine.snapshots()?})),
        Request::History => Ok(json!({"ok":true,"history":engine.history_status()?})),
        Request::Undo => Ok(json!({"ok":true,"changed":engine.undo()?})),
        Request::Redo => Ok(json!({"ok":true,"changed":engine.redo()?})),
        Request::Export => {
            Ok(json!({"ok":true,"data":serde_json::from_str::<Value>(&engine.export_json()?)?}))
        }
        Request::Import {
            data,
            require_empty,
        } => Ok(
            json!({"ok":true,"report":engine.import_json(&data.to_string(),mode(require_empty))?}),
        ),
        Request::ExportIcs => {
            let export = engine.export_ics()?;
            Ok(json!({"ok":true,"data":export.text,"notices":export.notices}))
        }
        Request::ImportIcs {
            data,
            require_empty,
        } => Ok(json!({"ok":true,"report":engine.import_ics(&data,mode(require_empty))?})),
        Request::Backup { filename } => {
            engine.backup_to(confined_file(&path, &filename)?)?;
            Ok(json!({"ok":true,"filename":filename}))
        }
        Request::Plan {
            timezone,
            enabled,
            missed_grace_minutes,
        } => {
            let mut policy = DevicePolicy {
                timezone,
                enabled,
                ..DevicePolicy::default()
            };
            if let Some(minutes) = missed_grace_minutes {
                policy.missed_grace_minutes = minutes;
            }
            Ok(
                json!({"ok":true,"notifications":plan_notifications(engine,&policy,chrono::Utc::now())?}),
            )
        }
        Request::Restore { .. } => unreachable!("restore handled before opening the live database"),
        Request::Network { .. } => unreachable!("network handled before acquiring the local owner"),
    })
}

fn mode(empty: bool) -> ImportMode {
    if empty {
        ImportMode::Empty
    } else {
        ImportMode::Merge
    }
}

fn database_path(input: &str) -> BridgeResult<PathBuf> {
    let path = Path::new(input);
    let name = path
        .file_name()
        .ok_or("host must provide a database filename")?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let path = fs::canonicalize(parent)?.join(name);
    reject_symlink(&path)?;
    Ok(path)
}

fn confined_file(database: &Path, name: &str) -> BridgeResult<PathBuf> {
    if name.is_empty()
        || name.len() > 128
        || !name.ends_with(".sqlite3")
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || !matches!(
            (
                Path::new(name).components().next(),
                Path::new(name).components().nth(1)
            ),
            (Some(Component::Normal(_)), None)
        )
    {
        return Err(
            "backup names must be a single 1..=128 byte ASCII filename ending in .sqlite3".into(),
        );
    }
    let parent = database
        .parent()
        .ok_or("host database has no parent directory")?;
    let path = parent.join(name);
    reject_symlink(&path)?;
    Ok(path)
}

fn reject_symlink(path: &Path) -> BridgeResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err("database/backup path must be a regular file, never a symlink".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
