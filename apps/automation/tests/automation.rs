use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::{Child, ChildStdin, ChildStdout, Command as Process},
    time::{Duration, timeout},
};
use ultratasks_automation::ai::{
    Plan, PlanningClock, Provider, ProviderConfig, ProviderKind, ScopeGuard, apply_plan,
};
use ultratasks_core::{Command, Engine, EntityId, Outcome, TaskQuery};

fn clock() -> PlanningClock {
    PlanningClock {
        now: Utc.with_ymd_and_hms(2026, 11, 9, 0, 0, 0).unwrap(),
        timezone: "Asia/Tokyo".into(),
    }
}

fn task(engine: &mut Engine, title: &str) -> ultratasks_core::Task {
    match engine
        .execute(Command::CreateTask {
            title: title.into(),
            due: None,
            list_id: None,
        })
        .unwrap()
    {
        Outcome::Task(task) => task,
        _ => panic!("task outcome"),
    }
}

struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpProcess {
    async fn start(path: &Path, writable: bool) -> Self {
        let mut command = Process::new(env!("CARGO_BIN_EXE_ultratasks-automation"));
        command.arg("--database").arg(path).arg("mcp");
        if writable {
            command.arg("--allow-writes");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut process = Self {
            child,
            stdin,
            stdout,
        };
        let result = process.request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"integration-test","version":"1"}}})).await;
        assert!(
            result.get("result").is_some(),
            "initialize failed: {result}"
        );
        process
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        process
    }
    async fn send(&mut self, value: Value) {
        self.stdin
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
        self.stdin.flush().await.unwrap();
    }
    async fn receive(&mut self) -> Value {
        let mut line = String::new();
        let count = timeout(Duration::from_secs(10), self.stdout.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        assert!(count > 0, "MCP unexpectedly closed stdout");
        serde_json::from_str(&line).expect("stdout must contain only protocol JSON")
    }
    async fn request(&mut self, value: Value) -> Value {
        self.send(value).await;
        self.receive().await
    }
    async fn tool(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await
    }
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }
}

#[tokio::test]
async fn stdio_mcp_read_only_denies_mutation_and_handles_malformed_messages() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.sqlite3");
    drop(Engine::open(&path).unwrap());
    let mut process = McpProcess::start(&path, false).await;
    let tools = process
        .request(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await;
    assert!(
        !tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "create_task")
    );
    let denied = process
        .tool(
            3,
            "create_task",
            json!({"request_id":"deny","title":"must not exist"}),
        )
        .await;
    assert!(denied.get("error").is_some() || denied["result"]["isError"] == true);
    process.stdin.write_all(b"{not-json}\n").await.unwrap();
    process.stdin.flush().await.unwrap();
    // Official rmcp ignores syntax-invalid input, then continues the session.
    // Well-formed JSON with an invalid JSON-RPC shape gets Invalid Request.
    let malformed = process
        .request(json!({"jsonrpc":"2.0","id":99,"method":10}))
        .await;
    assert_eq!(malformed["error"]["code"], -32600);
    let listed = process.tool(4, "search_tasks", json!({})).await;
    assert_eq!(listed["result"]["structuredContent"]["tasks"], json!([]));
    process.stop().await;
    assert!(
        Engine::open(&path)
            .unwrap()
            .tasks(&TaskQuery::default())
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn stdio_mcp_writes_retry_after_restart_and_reject_stale_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.sqlite3");
    drop(Engine::open(&path).unwrap());
    let create = json!({"request_id":"create-one","title":"日本語タスク"});
    let mut process = McpProcess::start(&path, true).await;
    let first = process.tool(2, "create_task", create.clone()).await;
    assert_eq!(first["result"]["isError"], false, "{first}");
    let task_id = first["result"]["structuredContent"]["outcomes"][0]["Task"]["id"].clone();
    process.stop().await;
    let mut process = McpProcess::start(&path, true).await;
    let repeated = process.tool(2, "create_task", create).await;
    assert_eq!(
        first["result"]["structuredContent"],
        repeated["result"]["structuredContent"]
    );
    let collision = process
        .tool(
            3,
            "create_task",
            json!({"request_id":"create-one","title":"different payload"}),
        )
        .await;
    assert_eq!(collision["result"]["isError"], true);
    let stale = process
        .tool(
            4,
            "complete_task",
            json!({"request_id":"stale","id":task_id,"expected_revision":2,"completed":true}),
        )
        .await;
    assert_eq!(stale["result"]["isError"], true);
    let unknown = process
        .tool(
            5,
            "create_task",
            json!({"request_id":"unknown","title":"bad","sql":"DROP TABLE tasks"}),
        )
        .await;
    assert!(unknown.get("error").is_some() || unknown["result"]["isError"] == true);
    process.stop().await;
    let tasks = Engine::open(&path)
        .unwrap()
        .tasks(&TaskQuery::default())
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(!tasks[0].completed);
}

async fn provider_server(draft: Value) -> (Provider, tokio::task::JoinHandle<Value>) {
    provider_server_kind(draft, ProviderKind::OpenaiCompatible).await
}

async fn provider_server_kind(
    draft: Value,
    kind: ProviderKind,
) -> (Provider, tokio::task::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse::<usize>().unwrap();
            }
        }
        assert!(content_length < 256 * 1024);
        let mut body = vec![0; content_length];
        stream.read_exact(&mut body).await.unwrap();
        let response = match kind {
            ProviderKind::OpenaiCompatible => {
                json!({"choices":[{"message":{"content":draft.to_string()}}]})
            }
            ProviderKind::Ollama => json!({"message":{"content":draft.to_string()}}),
        }
        .to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
        serde_json::from_slice(&body).unwrap()
    });
    let provider = Provider::new(ProviderConfig {
        provider: kind,
        endpoint: format!("http://{address}/v1/chat/completions"),
        model: "fixture-model".into(),
        api_key_env: None,
    })
    .unwrap();
    (provider, server)
}

#[tokio::test]
async fn ai_real_http_adapter_sends_only_selected_context_and_never_auto_applies() {
    let mut engine = Engine::in_memory().unwrap();
    let selected = task(&mut engine, "selected task");
    task(&mut engine, "PRIVATE_NOT_SELECTED");
    let (provider,server) = provider_server(json!({"summary":"Mark selected task done","commands":[{"SetTaskCompleted":{"id":selected.id,"expected_revision":1,"completed":true}}]})).await;
    let plan = provider
        .propose(
            &mut engine,
            "finish selected task",
            &[EntityId::Task(selected.id)],
            clock(),
        )
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert!(!request.to_string().contains("PRIVATE_NOT_SELECTED"));
    assert!(request.to_string().contains("Asia/Tokyo"));
    assert!(!engine.task(selected.id).unwrap().completed);
    assert_eq!(engine.task(selected.id).unwrap().revision, 1);
    let outcomes = apply_plan(&mut engine, &plan).unwrap();
    assert!(engine.task(selected.id).unwrap().completed);
    let repeated = apply_plan(&mut engine, &plan).unwrap();
    assert_eq!(
        serde_json::to_value(outcomes).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
    engine.undo().unwrap();
    assert!(!engine.task(selected.id).unwrap().completed);
}

#[tokio::test]
async fn ai_rejects_unselected_targets_without_changing_database() {
    let mut engine = Engine::in_memory().unwrap();
    let hidden = task(&mut engine, "hidden");
    let (provider,server) = provider_server(json!({"summary":"Unsafe target","commands":[{"DeleteTask":{"id":hidden.id,"expected_revision":1}}]})).await;
    assert!(
        provider
            .propose(&mut engine, "make a plan", &[], clock())
            .await
            .is_err()
    );
    server.await.unwrap();
    assert!(engine.task(hidden.id).is_ok());
}

#[test]
fn ai_context_conflict_and_partial_batch_are_atomic() {
    let mut engine = Engine::in_memory().unwrap();
    let selected = task(&mut engine, "context only");
    let plan = Plan {
        format_version: 1,
        request_id: "ai-context-conflict".into(),
        summary: "Create based on context".into(),
        clock: clock(),
        scope: vec![ScopeGuard {
            id: EntityId::Task(selected.id),
            revision: 1,
        }],
        commands: vec![Command::CreateTask {
            title: "new".into(),
            due: None,
            list_id: None,
        }],
    };
    engine
        .execute(Command::SetTaskCompleted {
            id: selected.id,
            expected_revision: 1,
            completed: true,
        })
        .unwrap();
    assert!(apply_plan(&mut engine, &plan).is_err());
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
    let mut broken = plan;
    broken.request_id = "ai-partial-failure".into();
    broken.scope[0].revision = 2;
    broken.commands.push(Command::DeleteTask {
        id: selected.id,
        expected_revision: 1,
    });
    assert!(apply_plan(&mut engine, &broken).is_err());
    assert_eq!(engine.tasks(&TaskQuery::default()).unwrap().len(), 1);
}

#[test]
fn provider_rejects_insecure_remote_and_credential_urls() {
    for endpoint in [
        "http://example.com/chat",
        "https://secret@example.com/chat",
        "https://example.com/chat?api_key=secret",
    ] {
        assert!(
            Provider::new(ProviderConfig {
                provider: ProviderKind::Ollama,
                endpoint: endpoint.into(),
                model: "test".into(),
                api_key_env: None
            })
            .is_err()
        );
    }
}

#[tokio::test]
async fn ollama_adapter_proposes_calendar_recurrence_and_reminder_as_one_undoable_action() {
    use ultratasks_core::{EventSchedule, Frequency, Recurrence};
    let mut engine = Engine::in_memory().unwrap();
    let start = chrono::NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
    let event = match engine
        .execute(Command::CreateEvent {
            title: "Morning".into(),
            schedule: EventSchedule::AllDay {
                start,
                end: start.succ_opt().unwrap(),
            },
        })
        .unwrap()
    {
        Outcome::Event(event) => event,
        _ => panic!("event"),
    };
    let calendar = match engine
        .execute(Command::CreateCalendar {
            title: "Old".into(),
        })
        .unwrap()
    {
        Outcome::Calendar(calendar) => calendar,
        _ => panic!("calendar"),
    };
    let commands = vec![
        Command::SetEventRecurrence {
            id: event.id,
            expected_revision: 1,
            recurrence: Some(Recurrence {
                frequency: Frequency::Daily,
                interval: 1,
                count: Some(3),
                until: None,
            }),
        },
        Command::RenameCalendar {
            id: calendar.id,
            expected_revision: 1,
            title: "Personal".into(),
        },
        Command::CreateReminder {
            target: EntityId::Event(event.id),
            trigger: ultratasks_core::ReminderTrigger::Before { minutes: 15 },
        },
    ];
    let (provider, server) = provider_server_kind(
        json!({"summary":"Repeat morning with a reminder","commands":commands}),
        ProviderKind::Ollama,
    )
    .await;
    let plan = provider
        .propose(
            &mut engine,
            "毎朝の予定に通知をつける",
            &[EntityId::Event(event.id), EntityId::Calendar(calendar.id)],
            clock(),
        )
        .await
        .unwrap();
    let request = server.await.unwrap();
    assert_eq!(request["stream"], false);
    assert_eq!(request["format"], "json");
    assert!(engine.event(event.id).unwrap().recurrence.is_none());
    apply_plan(&mut engine, &plan).unwrap();
    assert_eq!(
        engine.event(event.id).unwrap().recurrence.unwrap().count,
        Some(3)
    );
    assert_eq!(engine.calendar(calendar.id).unwrap().title, "Personal");
    assert_eq!(engine.reminders(100).unwrap().len(), 1);
    engine.undo().unwrap();
    assert!(engine.event(event.id).unwrap().recurrence.is_none());
    assert_eq!(engine.calendar(calendar.id).unwrap().title, "Old");
    assert!(engine.reminders(100).unwrap().is_empty());
}

#[test]
fn ai_cannot_delete_a_new_unselected_dependent_record() {
    let mut engine = Engine::in_memory().unwrap();
    let selected = task(&mut engine, "parent");
    let plan = Plan {
        format_version: 1,
        request_id: "delete-parent".into(),
        summary: "Delete selected parent".into(),
        clock: clock(),
        scope: vec![ScopeGuard {
            id: EntityId::Task(selected.id),
            revision: 1,
        }],
        commands: vec![Command::DeleteTask {
            id: selected.id,
            expected_revision: 1,
        }],
    };
    ultratasks_automation::ai::validate_plan(&mut engine, &plan).unwrap();
    let reminder = match engine
        .execute(Command::CreateReminder {
            target: EntityId::Task(selected.id),
            trigger: ultratasks_core::ReminderTrigger::At(clock().now),
        })
        .unwrap()
    {
        Outcome::Reminder(reminder) => reminder,
        _ => panic!("reminder"),
    };
    assert_eq!(engine.task(selected.id).unwrap().revision, 1);
    assert!(apply_plan(&mut engine, &plan).is_err());
    assert!(engine.task(selected.id).is_ok());
    assert!(engine.reminder(reminder.id).is_ok());
}

#[tokio::test]
async fn stdio_calendar_query_expands_recurring_events_beyond_seed_day() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.sqlite3");
    drop(Engine::open(&path).unwrap());
    let mut process = McpProcess::start(&path, true).await;
    let created = process.tool(2,"create_event",json!({"request_id":"event","title":"daily","schedule":{"AllDay":{"start":"2026-11-09","end":"2026-11-10"}}})).await;
    let id = created["result"]["structuredContent"]["outcomes"][0]["Event"]["id"].clone();
    let recurring = process.tool(3,"set_event_recurrence",json!({"request_id":"recurrence","id":id,"expected_revision":1,"recurrence":{"frequency":"Daily","interval":1,"count":10}})).await;
    assert_eq!(recurring["result"]["isError"], false, "{recurring}");
    let window = process
        .tool(
            4,
            "calendar_window",
            json!({"start":"2026-11-15T00:00:00Z","end":"2026-11-16T00:00:00Z","timezone":"UTC"}),
        )
        .await;
    let events = window["result"]["structuredContent"]["events"]
        .as_array()
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["original_date"], "2026-11-15");
    process.stop().await;
}

#[test]
fn cli_ai_apply_requires_explicit_approval() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.sqlite3");
    drop(Engine::open(&path).unwrap());
    let plan = Plan {
        format_version: 1,
        request_id: "cli-plan".into(),
        summary: "Add task".into(),
        clock: clock(),
        scope: vec![],
        commands: vec![Command::CreateTask {
            title: "review required".into(),
            due: None,
            list_id: None,
        }],
    };
    let plan_path = directory.path().join("plan.json");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ultratasks-automation"))
        .arg("--database")
        .arg(&path)
        .arg("ai-apply")
        .arg("--plan")
        .arg(&plan_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--approve"));
    assert!(
        Engine::open(path)
            .unwrap()
            .tasks(&TaskQuery::default())
            .unwrap()
            .is_empty()
    );
}
