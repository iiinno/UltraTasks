//! Local-first task and calendar engine. No GUI or network runtime is required.
//! Open an Engine on a dedicated worker thread; all writes are transactional.
mod engine;
mod ics;
mod model;
mod recurrence;
mod storage;
pub mod sync;
mod sync_capacity;
mod transfer;
pub use engine::{Engine, Error, Result};
pub use ics::IcsExport;
pub use model::*;
pub use recurrence::{expand_event, expand_task};
pub use storage::{Entity, Snapshot as RecordSnapshot};
pub use transfer::*;
