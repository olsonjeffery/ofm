#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod config;
mod db;
mod logging;
mod server;

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

    // Server
    let app = server::router();
    let addr = format!("{}:{}", cfg.hostname, cfg.port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
