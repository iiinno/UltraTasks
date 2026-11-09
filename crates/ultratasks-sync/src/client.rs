use crate::{Error, Result, SecretToken, Status};
use reqwest::{
    Method, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use ultratasks_core::{
    Engine,
    sync::{MAX_WIRE_BYTES, PROTOCOL_VERSION, SyncPull, SyncPushResult, SyncServerSnapshot},
};

pub struct Client {
    http: reqwest::Client,
    base: Url,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncProgress {
    pub pushed_batches: u64,
    pub pulled_batches: u64,
}
impl Client {
    pub fn new(endpoint: &str, token: SecretToken) -> Result<Self> {
        let mut base =
            Url::parse(endpoint).map_err(|_| Error::Configuration("invalid server URL".into()))?;
        let loopback = base.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if base.scheme() != "https" && !(base.scheme() == "http" && loopback) {
            return Err(Error::Configuration(
                "remote synchronization requires HTTPS; HTTP is allowed only on loopback".into(),
            ));
        }
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(Error::Configuration(
                "server URL must not contain credentials, query, or fragment".into(),
            ));
        }
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let mut headers = HeaderMap::new();
        let mut header = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
            .map_err(|_| Error::Configuration("invalid device credential".into()))?;
        header.set_sensitive(true);
        headers.insert(AUTHORIZATION, header);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .user_agent("UltraTasks-Sync/1")
            .build()?;
        Ok(Self { http, base })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        let url = self
            .base
            .join(path)
            .map_err(|_| Error::Configuration("invalid API path".into()))?;
        let mut request = self.http.request(method, url);
        if let Some(body) = body {
            request = request.json(body);
        }
        let mut response = request.send().await?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_WIRE_BYTES as u64)
        {
            return Err(Error::Configuration(
                "response exceeds synchronization size limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > MAX_WIRE_BYTES {
                return Err(Error::Configuration(
                    "response exceeds synchronization size limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            // Expose only the server's bounded message, never request headers/token.
            let message = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(|m| m.chars().take(500).collect())
                })
                .unwrap_or_else(|| "server request failed".into());
            return Err(Error::Remote {
                status: status.as_u16(),
                message,
            });
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn snapshot(&self) -> Result<SyncServerSnapshot> {
        self.request(Method::GET, "v1/snapshot", None::<&()>).await
    }

    /// The host serializes this with edits on its database worker. Durable requests
    /// make cancellation, lost responses and process restarts safe to retry.
    pub async fn sync_once(&self, engine: &mut Engine) -> Result<SyncProgress> {
        let status: Status = self.request(Method::GET, "v1/status", None::<&()>).await?;
        if status.protocol != PROTOCOL_VERSION {
            return Err(Error::Configuration("unsupported server protocol".into()));
        }
        if !engine.sync_attach(status.generation)? {
            // An earlier commit may have lost its response. Recover its original
            // receipt before comparing the new server incarnation to the baseline.
            if let Some(request) = engine.sync_prepared_request()? {
                let response: SyncPushResult = self
                    .request(Method::POST, "v1/push", Some(&request))
                    .await?;
                if matches!(response, SyncPushResult::Accepted { .. }) {
                    engine.sync_accept_push_result(&request, &response)?;
                }
            }
            let snapshot = self.snapshot().await?;
            if !engine.sync_recover_unchanged(&snapshot)? {
                return Err(Error::RecoveryRequired);
            }
        }
        if engine.sync_position()?.has_conflict {
            return Err(Error::Conflict);
        }
        let mut progress = SyncProgress::default();
        // Bound one invocation so a continuously editing writer cannot starve UI.
        for _ in 0..100 {
            let Some(request) = engine.sync_prepare_push()? else {
                break;
            };
            let response: SyncPushResult = self
                .request(Method::POST, "v1/push", Some(&request))
                .await?;
            match response {
                SyncPushResult::ResetRequired { .. } => return Err(Error::RecoveryRequired),
                SyncPushResult::Conflict { .. } => {
                    engine.sync_accept_push_result(&request, &response)?;
                    return Err(Error::Conflict);
                }
                SyncPushResult::Accepted { .. } => {
                    engine.sync_accept_push_result(&request, &response)?;
                    progress.pushed_batches += 1;
                }
            }
        }
        if engine.sync_position()?.pending_operations > 0 {
            return Ok(progress);
        }
        for _ in 0..100 {
            let position = engine.sync_position()?;
            let generation = position.generation.ok_or(Error::RecoveryRequired)?;
            let response: SyncPull = self
                .request(
                    Method::GET,
                    &format!(
                        "v1/pull?generation={generation}&cursor={}&limit=50",
                        position.cursor
                    ),
                    None::<&()>,
                )
                .await?;
            let more = response.has_more;
            progress.pulled_batches += response.batches.len() as u64;
            engine.sync_apply_pull(&response)?;
            if !more {
                break;
            }
        }
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_rejects_cleartext_remote_urls_and_embedded_credentials() {
        for endpoint in [
            "http://example.org",
            "http://192.168.1.2",
            "https://name:secret@example.org",
            "https://example.org/?token=x",
            "file:///tmp/db",
        ] {
            assert!(Client::new(endpoint, SecretToken::parse("a".repeat(64)).unwrap()).is_err());
        }
        for endpoint in [
            "http://127.0.0.1:8899",
            "http://[::1]:8899",
            "https://tasks.example.org",
        ] {
            assert!(Client::new(endpoint, SecretToken::parse("a".repeat(64)).unwrap()).is_ok());
        }
    }
}
