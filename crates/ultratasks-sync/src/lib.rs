//! Authenticated HTTP transport for the durable UltraTasks sync engine.
#[cfg(feature = "server")]
mod auth;
mod client;
#[cfg(feature = "server")]
mod server;
mod token;

#[cfg(feature = "server")]
pub use auth::{AuthStore, Device, DeviceCredential};
pub use client::{Client, SyncProgress};
#[cfg(feature = "server")]
pub use server::Server;
pub use token::SecretToken;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Status {
    pub protocol: u32,
    pub generation: uuid::Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Configuration(String),
    #[error("authentication failed")]
    Unauthorized,
    #[error("server rejected request ({status}): {message}")]
    Remote { status: u16, message: String },
    #[error("server state changed; explicit recovery is required")]
    RecoveryRequired,
    #[error("synchronization has an unresolved conflict")]
    Conflict,
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("storage error: {0}")]
    Storage(#[from] ultratasks_core::Error),
    #[cfg(feature = "server")]
    #[error("credential storage error: {0}")]
    Credentials(#[from] rusqlite::Error),
    #[error("invalid response: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("system error: {0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
