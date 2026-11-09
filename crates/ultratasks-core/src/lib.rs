//! Local-first task and calendar engine. No GUI or network runtime is required.
//! Open an Engine on a dedicated worker thread; all writes are transactional.
mod engine;
mod model;
mod storage;
pub use engine::{Engine, Error, Result};
pub use model::*;
