use anyhow::{Context, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, time::Duration};
use ultratasks_core::{Engine, sync::SyncResolution};
use ultratasks_sync::{Client, SecretToken};

#[derive(Parser)]
#[command(
    name = "ultratasks-sync",
    about = "Headless native UltraTasks synchronization client"
)]
struct Arguments {
    #[arg(long)]
    database: PathBuf,
    #[arg(long)]
    server: Option<String>,
    #[arg(long)]
    token_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Once,
    Watch {
        #[arg(long,default_value_t=30,value_parser=clap::value_parser!(u64).range(5..=86400))]
        interval_seconds: u64,
    },
    Status,
    Conflict,
    Resolve {
        #[arg(long, value_enum)]
        keep: Choice,
    },
    /// Re-capture a conflict after additional local edits, retaining the old alternatives.
    RefreshConflict,
    Recover {
        #[arg(long, value_enum)]
        keep: Choice,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum Choice {
    Local,
    Remote,
}
impl From<Choice> for SyncResolution {
    fn from(choice: Choice) -> Self {
        match choice {
            Choice::Local => Self::KeepLocal,
            Choice::Remote => Self::KeepRemote,
        }
    }
}
fn client(arguments: &Arguments) -> anyhow::Result<Client> {
    let endpoint = arguments
        .server
        .as_deref()
        .context("--server is required for network synchronization")?;
    let token_file = arguments
        .token_file
        .as_ref()
        .context("--token-file is required; tokens are never accepted on the command line")?;
    let value = std::fs::read_to_string(token_file).context("read the device credential file")?;
    Ok(Client::new(endpoint, SecretToken::parse(value.trim())?)?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    let mut engine = Engine::open_for_actor(&arguments.database, "sync-client")?;
    match arguments.command {
        Command::Once => {
            let progress = client(&arguments)?.sync_once(&mut engine).await?;
            println!(
                "Pushed {} batches; pulled {} batches",
                progress.pushed_batches, progress.pulled_batches
            );
        }
        Command::Watch { interval_seconds } => {
            let client = client(&arguments)?;
            let mut wait_seconds = interval_seconds;
            loop {
                tokio::select! {
                    _=tokio::signal::ctrl_c()=>break,
                    result=client.sync_once(&mut engine)=>match result {
                        Ok(progress)=>{ wait_seconds=interval_seconds;if progress.pushed_batches+progress.pulled_batches>0 { eprintln!("Synchronized: {} pushed, {} pulled",progress.pushed_batches,progress.pulled_batches); } }
                        Err(error)=>{ eprintln!("Synchronization deferred: {error}");wait_seconds=wait_seconds.saturating_mul(2).min(interval_seconds.max(300)); }
                    }
                }
                tokio::select! { _=tokio::signal::ctrl_c()=>break,_=tokio::time::sleep(Duration::from_secs(wait_seconds))=>{} }
            }
        }
        Command::Status => println!(
            "{}",
            serde_json::to_string_pretty(&engine.sync_position()?)?
        ),
        Command::Conflict => println!(
            "{}",
            serde_json::to_string_pretty(&engine.sync_conflict()?)?
        ),
        Command::Resolve { keep } => engine.sync_resolve_conflict(keep.into())?,
        Command::RefreshConflict => {
            if !engine.sync_position()?.has_conflict {
                bail!("there is no unresolved conflict");
            }
            engine.sync_refresh_conflict()?;
        }
        Command::Recover { keep } => {
            let guard = engine.sync_reset_guard()?;
            let snapshot = client(&arguments)?.snapshot().await?;
            engine.sync_resolve_reset_guarded(&snapshot, keep.into(), &guard)?;
        }
    }
    Ok(())
}
