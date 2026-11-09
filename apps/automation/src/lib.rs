//! Headless adapters. Every mutation uses the transactional core command API.
pub use ultratasks_ai as ai;
pub mod mcp;

use std::{fs::File, io::Read, path::Path};

/// Read a bounded document without trusting file metadata (pipes can change).
pub fn read_document(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut data)?;
    anyhow::ensure!(
        data.len() as u64 <= limit,
        "document exceeds the size limit"
    );
    Ok(data)
}
