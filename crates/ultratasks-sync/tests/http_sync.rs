use chrono::{TimeZone, Utc};
use std::path::Path;
use ultratasks_core::{
    Command, Engine, Outcome, Task, TaskQuery,
    sync::{SyncPushResult, SyncResolution},
};
use ultratasks_sync::{AuthStore, Client, Error, SecretToken, Server};

async fn serve(database: &Path, auth: &Path) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Server::open(database, auth).unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, server.router()).await.unwrap();
    });
    (format!("http://{address}"), handle)
}
fn task(engine: &mut Engine, title: &str) -> Task {
    match engine
        .execute(Command::CreateTask {
            title: title.into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    {
        Outcome::Task(value) => value,
        _ => unreachable!(),
    }
}
fn rename(engine: &mut Engine, task: &Task, title: &str) {
    engine
        .execute(Command::UpdateTask {
            id: task.id,
            expected_revision: task.revision,
            title: title.into(),
            completed: task.completed,
            due: task.due.clone(),
            list_id: task.list_id,
        })
        .unwrap();
}
fn credential(auth: &Path, name: &str) -> SecretToken {
    AuthStore::open(auth)
        .unwrap()
        .provision(name)
        .unwrap()
        .token
}

#[tokio::test]
async fn two_actual_http_clients_sync_offline_conflicts_and_tombstones() {
    let directory = tempfile::tempdir().unwrap();
    let auth = directory.path().join("auth.db");
    let server = directory.path().join("server.db");
    let token_a = credential(&auth, "windows");
    let token_b = credential(&auth, "android");
    let (endpoint, handle) = serve(&server, &auth).await;
    let ca = Client::new(&endpoint, token_a).unwrap();
    let cb = Client::new(&endpoint, token_b).unwrap();
    let mut a = Engine::open(directory.path().join("a.db")).unwrap();
    let mut b = Engine::open(directory.path().join("b.db")).unwrap();
    let initial = task(&mut a, "共通タスク");
    let block = match a
        .execute(Command::ScheduleTask {
            task_id: initial.id,
            expected_task_revision: initial.revision,
            start: Utc.with_ymd_and_hms(2026, 11, 9, 0, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 11, 9, 1, 0, 0).unwrap(),
            timezone: "Asia/Tokyo".into(),
        })
        .unwrap()
    {
        Outcome::TimeBlock(value) => value,
        _ => unreachable!(),
    };
    ca.sync_once(&mut a).await.unwrap();
    cb.sync_once(&mut b).await.unwrap();
    assert_eq!(b.task(initial.id).unwrap().title, "共通タスク");
    assert_eq!(b.time_block(block.id).unwrap().task_id, initial.id);
    let local_a = a.task(initial.id).unwrap();
    let local_b = b.task(initial.id).unwrap();
    rename(&mut a, &local_a, "Windowsの編集");
    rename(&mut b, &local_b, "Androidの編集");
    ca.sync_once(&mut a).await.unwrap();
    assert!(matches!(cb.sync_once(&mut b).await, Err(Error::Conflict)));
    let conflict = b.sync_conflict().unwrap().unwrap();
    assert!(
        serde_json::to_string(&conflict)
            .unwrap()
            .contains("Windowsの編集")
    );
    assert!(
        serde_json::to_string(&conflict)
            .unwrap()
            .contains("Androidの編集")
    );
    b.sync_resolve_conflict(SyncResolution::KeepLocal).unwrap();
    cb.sync_once(&mut b).await.unwrap();
    ca.sync_once(&mut a).await.unwrap();
    assert_eq!(a.task(initial.id).unwrap().title, "Androidの編集");
    let delete = a.task(initial.id).unwrap();
    a.execute(Command::DeleteTask {
        id: delete.id,
        expected_revision: delete.revision,
    })
    .unwrap();
    let edited = b.task(initial.id).unwrap();
    rename(&mut b, &edited, "削除と競合する編集");
    ca.sync_once(&mut a).await.unwrap();
    assert!(matches!(cb.sync_once(&mut b).await, Err(Error::Conflict)));
    b.sync_resolve_conflict(SyncResolution::KeepRemote).unwrap();
    cb.sync_once(&mut b).await.unwrap();
    assert!(b.task(initial.id).is_err());
    assert!(b.time_block(block.id).is_err());
    assert_eq!(b.tasks(&TaskQuery::default()).unwrap().len(), 0);
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn durable_receipt_survives_lost_response_client_restart_and_server_restart() {
    let directory = tempfile::tempdir().unwrap();
    let auth = directory.path().join("auth.db");
    let server = directory.path().join("server.db");
    let local = directory.path().join("local.db");
    let token = credential(&auth, "desktop");
    let (endpoint, handle) = serve(&server, &auth).await;
    let client = Client::new(&endpoint, token.clone()).unwrap();
    let mut engine = Engine::open(&local).unwrap();
    let initial = task(&mut engine, "一度だけ作成");
    let generation = client.snapshot().await.unwrap().generation;
    engine.sync_attach(generation).unwrap();
    let request = engine.sync_prepare_push().unwrap().unwrap();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{endpoint}/v1/push"))
        .bearer_auth(token.expose())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let committed: SyncPushResult = response.json().await.unwrap();
    assert!(matches!(committed, SyncPushResult::Accepted { .. }));
    // Intentionally do not acknowledge the response locally.
    drop(engine);
    handle.abort();
    let _ = handle.await;
    let (endpoint, handle) = serve(&server, &auth).await;
    let client = Client::new(&endpoint, token).unwrap();
    let mut engine = Engine::open(&local).unwrap();
    client.sync_once(&mut engine).await.unwrap();
    assert_eq!(engine.sync_position().unwrap().pending_operations, 0);
    let snapshot = client.snapshot().await.unwrap();
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].revision(), 1);
    assert_eq!(engine.task(initial.id).unwrap().title, "一度だけ作成");
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn rollback_is_detected_without_dropping_local_pending_changes() {
    let directory = tempfile::tempdir().unwrap();
    let auth = directory.path().join("auth.db");
    let server = directory.path().join("server.db");
    let token = credential(&auth, "desktop");
    let (endpoint, handle) = serve(&server, &auth).await;
    let client = Client::new(&endpoint, token.clone()).unwrap();
    let mut engine = Engine::open(directory.path().join("local.db")).unwrap();
    let previous = task(&mut engine, "確認済みデータ");
    client.sync_once(&mut engine).await.unwrap();
    let queued = task(&mut engine, "オフラインの未送信データ");
    handle.abort();
    let _ = handle.await;
    // A fresh server represents restoration to a backup predating both records.
    let (endpoint, handle) = serve(&directory.path().join("restored.db"), &auth).await;
    let client = Client::new(&endpoint, token).unwrap();
    assert!(matches!(
        client.sync_once(&mut engine).await,
        Err(Error::RecoveryRequired)
    ));
    assert!(engine.task(previous.id).is_ok());
    assert!(engine.task(queued.id).is_ok());
    assert!(engine.sync_position().unwrap().pending_operations > 0);
    let snapshot = client.snapshot().await.unwrap();
    engine
        .sync_resolve_reset(&snapshot, SyncResolution::KeepLocal)
        .unwrap();
    client.sync_once(&mut engine).await.unwrap();
    assert_eq!(client.snapshot().await.unwrap().records.len(), 2);
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn device_revocation_and_unauthenticated_requests_are_enforced_over_http() {
    let directory = tempfile::tempdir().unwrap();
    let auth = directory.path().join("auth.db");
    let mut store = AuthStore::open(&auth).unwrap();
    let credential = store.provision("revocable").unwrap();
    let (endpoint, handle) = serve(&directory.path().join("server.db"), &auth).await;
    let client = Client::new(&endpoint, credential.token).unwrap();
    assert!(client.snapshot().await.is_ok());
    store.revoke(credential.device.id).unwrap();
    assert!(matches!(client.snapshot().await, Err(Error::Unauthorized)));
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{endpoint}/v1/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn an_existing_local_dataset_can_seed_a_dedicated_server_and_log_later_edits() {
    let directory = tempfile::tempdir().unwrap();
    let auth = directory.path().join("auth.db");
    let server_database = directory.path().join("server.db");
    let token = credential(&auth, "new-device");
    let mut source = Engine::open(&server_database).unwrap();
    let initial = task(&mut source, "既存のデータ");
    drop(source);
    let (endpoint, handle) = serve(&server_database, &auth).await;
    let client = Client::new(&endpoint, token).unwrap();
    let mut device = Engine::open(directory.path().join("device.db")).unwrap();
    client.sync_once(&mut device).await.unwrap();
    assert_eq!(device.task(initial.id).unwrap().title, "既存のデータ");
    let mut administrator = Engine::open(&server_database).unwrap();
    let current = administrator.task(initial.id).unwrap();
    rename(&mut administrator, &current, "サーバー上の編集");
    client.sync_once(&mut device).await.unwrap();
    assert_eq!(device.task(initial.id).unwrap().title, "サーバー上の編集");
    assert!(
        administrator
            .sync_attach(client.snapshot().await.unwrap().generation)
            .is_err()
    );
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn redirects_never_forward_device_credentials_to_another_endpoint() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let received = Arc::new(AtomicUsize::new(0));
    let counter = received.clone();
    let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let target_task = tokio::spawn(async move {
        let router = axum::Router::new().fallback(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                "unexpected forwarded request"
            }
        });
        axum::serve(target, router).await.unwrap();
    });
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_address = source.local_addr().unwrap();
    let source_task = tokio::spawn(async move {
        let router = axum::Router::new().fallback(move || async move {
            axum::response::Redirect::temporary(&format!("http://{target_address}/capture"))
        });
        axum::serve(source, router).await.unwrap();
    });
    let client = Client::new(
        &format!("http://{source_address}"),
        SecretToken::parse("a".repeat(64)).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        client.snapshot().await,
        Err(Error::Remote { status: 307, .. })
    ));
    assert_eq!(received.load(Ordering::SeqCst), 0);
    source_task.abort();
    target_task.abort();
    let _ = source_task.await;
    let _ = target_task.await;
}
