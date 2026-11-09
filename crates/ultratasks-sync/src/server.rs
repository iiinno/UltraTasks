use crate::{AuthStore, Error, Result, SecretToken, Status};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
type RequestPermit = Arc<OwnedSemaphorePermit>;
use ultratasks_core::{
    Engine,
    sync::{MAX_WIRE_BYTES, PROTOCOL_VERSION, SyncPush},
};
use uuid::Uuid;

#[derive(Clone)]
struct ServerState {
    engine: Arc<Mutex<Engine>>,
    auth: Arc<Mutex<AuthStore>>,
    permits: Arc<Semaphore>,
}

pub struct Server {
    state: ServerState,
}
impl Server {
    pub fn open(database: impl AsRef<Path>, credentials: impl AsRef<Path>) -> Result<Self> {
        let auth = AuthStore::open(credentials)?;
        let mut engine = Engine::open_for_actor(database, "sync-server")?;
        engine.sync_server_start()?;
        Ok(Self {
            state: ServerState {
                engine: Arc::new(Mutex::new(engine)),
                auth: Arc::new(Mutex::new(auth)),
                permits: Arc::new(Semaphore::new(4)),
            },
        })
    }
    pub fn router(&self) -> Router {
        Router::new()
            .route("/v1/status", get(status))
            .route("/v1/snapshot", get(snapshot))
            .route("/v1/pull", get(pull))
            .route("/v1/push", post(push))
            .layer(DefaultBodyLimit::max(MAX_WIRE_BYTES))
            .layer(middleware::from_fn_with_state(
                self.state.clone(),
                authenticate,
            ))
            .route("/health", get(|| async { "ok" }))
            .with_state(self.state.clone())
    }
}

#[derive(Deserialize)]
struct PullQuery {
    generation: Uuid,
    cursor: u64,
    #[serde(default = "default_page")]
    limit: u32,
}
fn default_page() -> u32 {
    50
}

fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({"error":code,"message":message})),
    )
        .into_response()
}
fn core_failure(error: ultratasks_core::Error) -> Response {
    match error {
        ultratasks_core::Error::Database(_) | ultratasks_core::Error::Serialization(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "request could not be committed",
        ),
        other => failure(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            &other.to_string(),
        ),
    }
}
async fn authenticate(
    State(state): State<ServerState>,
    mut request: Request,
    next: Next,
) -> Response {
    let permit = match state.permits.clone().try_acquire_owned() {
        Ok(permit) => Arc::new(permit),
        Err(_) => return failure(StatusCode::TOO_MANY_REQUESTS, "busy", "retry later"),
    };
    let token = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|v| SecretToken::parse(v).ok());
    let Some(token) = token else {
        return failure(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid device credential is required",
        );
    };
    let auth = state.auth.clone();
    let auth_permit = permit.clone();
    let authorized = tokio::task::spawn_blocking(move || {
        let _permit = auth_permit;
        auth.lock()
            .map_err(|_| Error::Configuration("credential lock poisoned".into()))?
            .authenticate(&token)
    })
    .await;
    if !matches!(authorized, Ok(Ok(Some(_)))) {
        return failure(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid device credential is required",
        );
    }
    request.extensions_mut().insert(permit.clone());
    let result = tokio::time::timeout(Duration::from_secs(30), next.run(request)).await;
    drop(permit);
    result.unwrap_or_else(|_| {
        failure(
            StatusCode::REQUEST_TIMEOUT,
            "timeout",
            "retry the same operation ID",
        )
    })
}
async fn with_engine<T, F>(state: ServerState, permit: RequestPermit, work: F) -> Response
where
    T: Serialize + Send + 'static,
    F: FnOnce(&mut Engine) -> ultratasks_core::Result<T> + Send + 'static,
{
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut engine = state.engine.lock().map_err(|_| {
            ultratasks_core::Error::InvalidInput("server engine unavailable".into())
        })?;
        work(&mut engine)
    })
    .await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(error)) => core_failure(error),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker_error",
            "request could not be completed",
        ),
    }
}
async fn status(
    State(state): State<ServerState>,
    Extension(permit): Extension<RequestPermit>,
) -> Response {
    with_engine(state, permit, |engine| {
        Ok(Status {
            protocol: PROTOCOL_VERSION,
            generation: engine.sync_server_generation()?,
        })
    })
    .await
}
async fn snapshot(
    State(state): State<ServerState>,
    Extension(permit): Extension<RequestPermit>,
) -> Response {
    with_engine(state, permit, Engine::sync_server_snapshot).await
}
async fn pull(
    State(state): State<ServerState>,
    Extension(permit): Extension<RequestPermit>,
    Query(query): Query<PullQuery>,
) -> Response {
    with_engine(state, permit, move |engine| {
        engine.sync_server_pull(query.generation, query.cursor, query.limit)
    })
    .await
}
async fn push(
    State(state): State<ServerState>,
    Extension(permit): Extension<RequestPermit>,
    Json(request): Json<SyncPush>,
) -> Response {
    with_engine(state, permit, move |engine| {
        engine.sync_server_push(&request)
    })
    .await
}
