use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand};
use rmcp::{ServiceExt, transport::async_rw::AsyncRwTransport};
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use ultratasks_automation::{
    ai::{Plan, PlanningClock, Provider, ProviderConfig, apply_plan},
    mcp::McpServer,
    read_document,
};
use ultratasks_core::{CalendarQuery, Command, Engine, EntityId, ImportMode, TaskQuery};

#[derive(Parser)]
#[command(
    version,
    about = "UltraTasks headless calendar, task, MCP and AI tools"
)]
struct Cli {
    #[arg(long)]
    database: PathBuf,
    #[arg(long, global = true, default_value = "automation")]
    actor: String,
    #[command(subcommand)]
    action: Action,
}

#[derive(Args)]
struct AiProposeArgs {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    prompt_file: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    timezone: String,
    #[arg(long)]
    task: Vec<ultratasks_core::TaskId>,
    #[arg(long)]
    event: Vec<ultratasks_core::EventId>,
    #[arg(long)]
    list: Vec<ultratasks_core::ListId>,
    #[arg(long)]
    time_block: Vec<ultratasks_core::TimeBlockId>,
    #[arg(long)]
    calendar: Vec<ultratasks_core::CalendarId>,
    #[arg(long)]
    reminder: Vec<ultratasks_core::ReminderId>,
}

#[derive(Subcommand)]
enum Action {
    /// Create/open a local database and run migrations.
    Init,
    /// Search tasks. A missing database is an error, never silently created.
    Tasks {
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        completed: Option<bool>,
        #[arg(long)]
        list: Option<ultratasks_core::ListId>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Read one bounded calendar window (RFC3339 boundaries, IANA timezone).
    Calendar {
        #[arg(long)]
        start: chrono::DateTime<chrono::Utc>,
        #[arg(long)]
        end: chrono::DateTime<chrono::Utc>,
        #[arg(long)]
        timezone: String,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// Apply a JSON array of typed core commands as one atomic, undoable action.
    Command {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        request_id: String,
    },
    /// Validate commands in a rolled-back transaction; make no saved changes.
    PreviewCommand {
        #[arg(long)]
        file: PathBuf,
    },
    Undo,
    Redo,
    History,
    ExportJson {
        #[arg(long)]
        output: PathBuf,
    },
    ImportJson {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        merge: bool,
    },
    ExportIcs {
        #[arg(long)]
        output: PathBuf,
    },
    ImportIcs {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        merge: bool,
    },
    Backup {
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate and restore a backup into --database, which must not exist.
    Restore {
        #[arg(long)]
        input: PathBuf,
    },
    /// MCP over stdin/stdout. Read-only tools by default; no network listener.
    Mcp {
        #[arg(long)]
        allow_writes: bool,
    },
    /// Send only the prompt and explicitly selected IDs to the configured AI.
    AiPropose(Box<AiProposeArgs>),
    /// Apply a reviewed AI plan. Requires explicit --approve; never calls a model.
    AiApply {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        approve: bool,
    },
}

fn read_text(path: &Path, limit: u64) -> Result<String> {
    String::from_utf8(read_document(path, limit)?).context("document must be UTF-8")
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file =
        tempfile::NamedTempFile::new_in(parent).context("output directory must be writable")?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path)
        .context("output must be a new writable file")?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn mode(merge: bool) -> ImportMode {
    if merge {
        ImportMode::Merge
    } else {
        ImportMode::Empty
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Action::Restore { input } = cli.action {
        Engine::restore_backup(input, &cli.database)?;
        println!("{}", json!({"restored":true}));
        return Ok(());
    }
    if !matches!(cli.action, Action::Init) {
        ensure!(
            cli.database.is_file(),
            "database does not exist; run init explicitly"
        );
    }
    let mut engine = if matches!(
        cli.action,
        Action::Mcp {
            allow_writes: false
        }
    ) {
        Engine::open_read_only(&cli.database)?
    } else {
        Engine::open_for_actor(&cli.database, cli.actor)?
    };
    let result = match cli.action {
        Action::Init => json!({"initialized":true}),
        Action::Tasks {
            search,
            completed,
            list,
            limit,
            offset,
        } => {
            json!({"tasks":engine.tasks(&TaskQuery { search, completed, list_id:list, limit, offset })?})
        }
        Action::Calendar {
            start,
            end,
            timezone,
            limit,
        } => {
            let query = CalendarQuery {
                start,
                end,
                timezone,
                limit,
            };
            json!({"events":engine.event_occurrences(&query,None)?,"task_occurrences":engine.task_occurrences(&query)?,"time_blocks":engine.time_blocks(&query)?})
        }
        Action::Command { file, request_id } => {
            let commands: Vec<Command> =
                serde_json::from_slice(&read_document(&file, 1024 * 1024)?)?;
            json!({"outcomes":engine.execute_idempotent(&request_id, commands)?})
        }
        Action::PreviewCommand { file } => {
            let commands: Vec<Command> =
                serde_json::from_slice(&read_document(&file, 1024 * 1024)?)?;
            json!({"preview":engine.preview_batch(commands)?})
        }
        Action::Undo => json!({"changed":engine.undo()?}),
        Action::Redo => json!({"changed":engine.redo()?}),
        Action::History => {
            let status = engine.history_status()?;
            json!({"can_undo":status.can_undo,"can_redo":status.can_redo})
        }
        Action::ExportJson { output } => {
            write_new(&output, engine.export_json()?.as_bytes())?;
            json!({"exported":true})
        }
        Action::ImportJson { input, merge } => {
            let report = engine.import_json(&read_text(&input, 32 * 1024 * 1024)?, mode(merge))?;
            json!({"inserted":report.inserted,"unchanged":report.unchanged})
        }
        Action::ExportIcs { output } => {
            let export = engine.export_ics()?;
            for notice in &export.notices {
                eprintln!("{notice}");
            }
            write_new(&output, export.text.as_bytes())?;
            json!({"exported":true,"notices":export.notices})
        }
        Action::ImportIcs { input, merge } => {
            let report = engine.import_ics(&read_text(&input, 32 * 1024 * 1024)?, mode(merge))?;
            json!({"inserted":report.inserted,"unchanged":report.unchanged})
        }
        Action::Backup { output } => {
            engine.backup_to(output)?;
            json!({"backed_up":true})
        }
        Action::Restore { .. } => unreachable!(),
        Action::Mcp { allow_writes } => {
            let transport = AsyncRwTransport::new(tokio::io::stdin(), tokio::io::stdout())
                .with_max_line_length(256 * 1024);
            McpServer::new(engine, allow_writes)
                .serve(transport)
                .await?
                .waiting()
                .await?;
            return Ok(());
        }
        Action::AiPropose(args) => {
            let AiProposeArgs {
                config,
                prompt_file,
                output,
                timezone,
                task,
                event,
                list,
                time_block,
                calendar,
                reminder,
            } = *args;
            let config: ProviderConfig =
                serde_json::from_slice(&read_document(&config, 16 * 1024)?)?;
            let provider = Provider::new(config)?;
            let ids: Vec<_> = task
                .into_iter()
                .map(EntityId::Task)
                .chain(event.into_iter().map(EntityId::Event))
                .chain(list.into_iter().map(EntityId::List))
                .chain(time_block.into_iter().map(EntityId::TimeBlock))
                .chain(calendar.into_iter().map(EntityId::Calendar))
                .chain(reminder.into_iter().map(EntityId::Reminder))
                .collect();
            let plan = provider
                .propose(
                    &mut engine,
                    &read_text(&prompt_file, 16 * 1024)?,
                    &ids,
                    PlanningClock {
                        now: chrono::Utc::now(),
                        timezone,
                    },
                )
                .await?;
            write_new(&output, &serde_json::to_vec_pretty(&plan)?)?;
            json!({"proposal_saved":true,"applied":false,"commands":plan.commands.len(),"request_id":plan.request_id})
        }
        Action::AiApply { plan, approve } => {
            ensure!(approve, "review the plan and pass --approve to apply it");
            let plan: Plan = serde_json::from_slice(&read_document(&plan, 1024 * 1024)?)?;
            json!({"outcomes":apply_plan(&mut engine, &plan)?})
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
