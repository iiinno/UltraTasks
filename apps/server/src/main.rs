use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use std::{io::Write, net::SocketAddr, path::PathBuf};
use ultratasks_core::{Engine, sync::SyncResolution};
use ultratasks_sync::{AuthStore, Client, SecretToken, Server};

#[derive(Parser)]
#[command(
    name = "ultratasks-server",
    about = "Personal UltraTasks synchronization server"
)]
struct Arguments {
    #[arg(long, default_value = "ultratasks-server.db")]
    database: PathBuf,
    #[arg(long, default_value = "ultratasks-devices.db")]
    credentials: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long, default_value = "127.0.0.1:8374")]
        listen: SocketAddr,
        /// Required when a private container interface is exposed to a TLS proxy.
        #[arg(long)]
        behind_tls_proxy: bool,
    },
    AddDevice {
        #[arg(long)]
        name: String,
        #[arg(long)]
        token_output: PathBuf,
    },
    RevokeDevice {
        id: String,
    },
    ListDevices,
    /// Synchronize --database as a local client. Credentials are read from a file.
    Sync {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        token_file: PathBuf,
    },
    /// Resolve the currently persisted ordinary conflict, retaining both alternatives.
    ResolveConflict {
        #[arg(long,value_parser=["local","remote"])]
        keep: String,
    },
    /// Explicitly recover after server rollback/divergence, archiving both sides.
    ResolveReset {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long,value_parser=["local","remote"])]
        keep: String,
    },
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = terminate.recv() => {},
                }
            }
            Err(error) => {
                eprintln!("Could not register SIGTERM handler: {error}");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Arguments::parse();
    match args.command {
        Command::Serve {
            listen,
            behind_tls_proxy,
        } => {
            if !listen.ip().is_loopback() && !behind_tls_proxy {
                bail!(
                    "non-loopback listeners require --behind-tls-proxy and a private network with a TLS reverse proxy"
                );
            }
            let listener = tokio::net::TcpListener::bind(listen).await?;
            let server = Server::open(&args.database, &args.credentials)?;
            eprintln!("UltraTasks sync listening on {}", listener.local_addr()?);
            axum::serve(listener, server.router())
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
        Command::AddDevice { name, token_output } => {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options
                .open(&token_output)
                .context("credential output must be a new file in a private directory")?;
            let result = (|| -> anyhow::Result<_> {
                let mut store = AuthStore::open(&args.credentials)?;
                let credential = store.provision(&name)?;
                if let Err(error) = output
                    .write_all(credential.token.expose().as_bytes())
                    .and_then(|_| output.sync_all())
                {
                    let _ = store.revoke(credential.device.id);
                    return Err(error.into());
                }
                Ok(credential.device.id)
            })();
            match result {
                Ok(id) => println!(
                    "Device {id} registered; credential saved to {}",
                    token_output.display()
                ),
                Err(error) => {
                    drop(output);
                    let _ = std::fs::remove_file(&token_output);
                    return Err(error);
                }
            }
        }
        Command::RevokeDevice { id } => {
            let id = id.parse().context("device ID must be a UUID")?;
            if !AuthStore::open(&args.credentials)?.revoke(id)? {
                bail!("device does not exist or is already revoked");
            }
        }
        Command::ListDevices => {
            for device in AuthStore::open(&args.credentials)?.devices()? {
                println!(
                    "{}\t{}\t{}",
                    device.id,
                    if device.revoked { "revoked" } else { "active" },
                    device.name
                );
            }
        }
        Command::Sync {
            endpoint,
            token_file,
        } => {
            let token = SecretToken::parse(std::fs::read_to_string(token_file)?.trim())?;
            let client = Client::new(&endpoint, token)?;
            let mut engine = Engine::open(&args.database)?;
            let progress = client.sync_once(&mut engine).await?;
            println!(
                "Pushed {} batches; pulled {} batches",
                progress.pushed_batches, progress.pulled_batches
            );
        }
        Command::ResolveConflict { keep } => {
            Engine::open(&args.database)?.sync_resolve_conflict(if keep == "local" {
                SyncResolution::KeepLocal
            } else {
                SyncResolution::KeepRemote
            })?;
        }
        Command::ResolveReset {
            endpoint,
            token_file,
            keep,
        } => {
            let token = SecretToken::parse(std::fs::read_to_string(token_file)?.trim())?;
            let client = Client::new(&endpoint, token)?;
            let mut engine = Engine::open(&args.database)?;
            let guard = engine.sync_reset_guard()?;
            let snapshot = client.snapshot().await?;
            engine.sync_resolve_reset_guarded(
                &snapshot,
                if keep == "local" {
                    SyncResolution::KeepLocal
                } else {
                    SyncResolution::KeepRemote
                },
                &guard,
            )?;
        }
    }
    Ok(())
}
