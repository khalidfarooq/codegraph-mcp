use clap::Parser;
use codegraph_security::auth::get_socket_path_for_repo;
use codegraphd::DaemonServer;
use fs4::fs_std::FileExt;
use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "codegraphd", about = "CodeGraph singleton background daemon")]
pub struct DaemonArgs {
    #[arg(short, long, default_value = ".")]
    pub repo: PathBuf,

    #[arg(short, long)]
    pub socket: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub untrusted: bool,
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = DaemonArgs::parse();
    let repo_root = dunce::canonicalize(&args.repo).unwrap_or(args.repo);

    let socket_path = match args.socket {
        Some(s) => s,
        None => get_socket_path_for_repo(&repo_root)?,
    };

    // 1. Advisory Lockfile Protection to prevent multiple daemons on same repo
    let lockfile_path = socket_path.with_extension("pid.lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lockfile_path)?;

    let is_locked = lock_file.try_lock_exclusive().is_ok();
    if !is_locked {
        error!(
            "Another codegraphd instance is already running for this repository (lock: {})",
            lockfile_path.display()
        );
        std::process::exit(1);
    }

    info!("Starting codegraphd daemon for: {}", repo_root.display());
    let server = Arc::new(DaemonServer::new(repo_root, args.untrusted));

    // Initial indexing
    server.index_repository().await;

    // Run IPC Unix Socket server
    server.run(socket_path).await?;

    Ok(())
}
