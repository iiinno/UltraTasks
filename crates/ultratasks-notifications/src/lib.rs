//! Durable, device-local reconciliation with native OS notification queues.
//! Desired reminders live in the shared core; delivery state never synchronizes.
mod planner;
mod scheduler;
#[cfg(windows)]
mod windows;

pub use planner::{DevicePolicy, plan_notifications};
pub use scheduler::{NotificationBackend, ReconcileReport, Scheduler};
#[cfg(windows)]
pub use windows::WindowsBackend;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredNotification {
    /// Stable reminder + original occurrence identity, never its mutable time.
    pub id: String,
    pub fire_at: DateTime<Utc>,
    pub title: String,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid notification configuration: {0}")]
    Invalid(String),
    #[error("notification permission unavailable: {0}")]
    Permission(String),
    #[error("native notification operation failed: {0}")]
    Platform(String),
    #[error("notification state is not an UltraTasks device database")]
    UnrecognizedDatabase,
    #[error("notification state belongs to another device")]
    WrongDevice,
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Core(#[from] ultratasks_core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
