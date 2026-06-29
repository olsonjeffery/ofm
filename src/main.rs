#![allow(dead_code)]

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod archive;
mod config;
mod db;
mod logging;
mod omp;
mod server;
mod services;
mod worktree;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();

    let cfg = config::OmprintConfig::from_env();

    // DB setup
    std::fs::create_dir_all(&cfg.data_dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(&cfg.data_dir, std::fs::Permissions::from_mode(0o700))?;

    let node_config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: "127.0.0.1:0".into(),
            addr_api: "127.0.0.1:0".into(),
        }],
        data_dir: cfg.data_dir.clone().into(),
        secret_raft: std::env::var("OMPRINT_RAFT_SECRET")
            .unwrap_or_else(|_| "omprint-raft-secret".into()),
        secret_api: std::env::var("OMPRINT_API_SECRET")
            .unwrap_or_else(|_| "omprint-api-secret".into()),
        ..Default::default()
    };

    let client = hiqlite::start_node(node_config).await?;
    client.wait_until_healthy_db().await;

    let count = db::run_migrations(&client).await?;
    tracing::info!("Migrations complete: {} applied", count);

    let default_user_id = db::ensure_default_user(&client).await?;
    tracing::info!("Default user id: {}", default_user_id);

    let state = server::state::AppState {
        db: client,
        default_user_id,
        archive_root: cfg.archive_root,
    };

    // Server
    let app = server::router(state);
    let addr = format!("{}:{}", cfg.hostname, cfg.port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
