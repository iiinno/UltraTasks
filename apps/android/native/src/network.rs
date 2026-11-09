//! Native-host network operations. The JNI host must invoke these on a worker.
//! A separate guarded SQLite connection leaves the local engine available while
//! a network request is in flight. Credentials are caller-owned and not persisted.
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use ultratasks_ai::{Plan, PlanningClock, Provider, ProviderConfig, apply_plan};
use ultratasks_core::{Engine, EntityId, sync::SyncResolution};
use ultratasks_sync::{Client, SecretToken};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkRequest {
    Sync {
        endpoint: String,
        token: String,
    },
    SyncStatus,
    SyncConflict,
    ResolveConflict {
        resolution: SyncResolution,
        approved: bool,
    },
    RecoverServer {
        endpoint: String,
        token: String,
        resolution: SyncResolution,
        approved: bool,
    },
    AiPropose {
        config: ProviderConfig,
        api_key: Option<String>,
        prompt: String,
        selected: Vec<EntityId>,
        timezone: String,
    },
    AiApply {
        plan: Plan,
        approved: bool,
    },
}

pub fn dispatch(path: &Path, request: NetworkRequest) -> Result<Value, Box<dyn std::error::Error>> {
    let mut engine = Engine::open_for_actor(path, "android")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    match request {
        NetworkRequest::Sync { endpoint, token } => {
            let client = Client::new(&endpoint, SecretToken::parse(token)?)?;
            let progress = runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    client.sync_once(&mut engine),
                )
                .await
            })??;
            Ok(
                json!({"ok":true,"pushed_batches":progress.pushed_batches,"pulled_batches":progress.pulled_batches,"position":engine.sync_position()?}),
            )
        }
        NetworkRequest::SyncStatus => Ok(json!({"ok":true,"position":engine.sync_position()?})),
        NetworkRequest::SyncConflict => Ok(json!({"ok":true,"conflict":engine.sync_conflict()?})),
        NetworkRequest::ResolveConflict {
            resolution,
            approved,
        } => {
            require_approval(approved)?;
            engine.sync_resolve_conflict(resolution)?;
            Ok(json!({"ok":true,"position":engine.sync_position()?}))
        }
        NetworkRequest::RecoverServer {
            endpoint,
            token,
            resolution,
            approved,
        } => {
            require_approval(approved)?;
            let client = Client::new(&endpoint, SecretToken::parse(token)?)?;
            let guard = engine.sync_reset_guard()?;
            let remote = runtime.block_on(client.snapshot())?;
            engine.sync_resolve_reset_guarded(&remote, resolution, &guard)?;
            Ok(json!({"ok":true,"position":engine.sync_position()?}))
        }
        NetworkRequest::AiPropose {
            config,
            api_key,
            prompt,
            selected,
            timezone,
        } => {
            let provider = Provider::with_api_key(config, api_key)?;
            let clock = PlanningClock {
                now: chrono::Utc::now(),
                timezone,
            };
            let plan =
                runtime.block_on(provider.propose(&mut engine, &prompt, &selected, clock))?;
            Ok(json!({"ok":true,"plan":plan}))
        }
        NetworkRequest::AiApply { plan, approved } => {
            require_approval(approved)?;
            Ok(json!({"ok":true,"outcomes":apply_plan(&mut engine,&plan)?}))
        }
    }
}

fn require_approval(approved: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !approved {
        return Err("explicit approval is required for this operation".into());
    }
    Ok(())
}
