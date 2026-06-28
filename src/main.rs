#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

mod archive;
mod config;
mod db;
mod logging;
mod server;
mod services;
mod worktree;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();

    let cfg = config::OmprintConfig::from_env();

    // DB setup
    if let Some(parent) = std::path::Path::new(&cfg.db_path).parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let conn = rusqlite::Connection::open(&cfg.db_path)?;
    #[cfg(unix)]
    std::fs::set_permissions(&cfg.db_path, std::fs::Permissions::from_mode(0o600))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    let count = db::run_migrations(&conn)?;
    tracing::info!("Migrations complete: {} applied", count);

    // Ensure a default user exists (auth not yet implemented)
    let default_user_id = db::ensure_default_user(&conn)?;
    tracing::info!("Default user id: {}", default_user_id);

    let state = server::state::AppState {
        db: Arc::new(Mutex::new(conn)),
        default_user_id,
    };

    // Server
    let app = server::router(state);
    let addr = format!("{}:{}", cfg.hostname, cfg.port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
