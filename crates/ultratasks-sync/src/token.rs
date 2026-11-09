use crate::{Error, Result};
use std::fmt;

/// Deliberately redacted in logs and diagnostics. Persist in the OS credential store
/// or a private file; the synchronization database never stores the plaintext token.
#[derive(Clone)]
pub struct SecretToken(String);
impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken([REDACTED])")
    }
}
impl SecretToken {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::Configuration(
                "device token must contain 64 lowercase hexadecimal characters".into(),
            ));
        }
        Ok(Self(value))
    }
    /// Explicit access for credential-file creation or authorization headers only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}
