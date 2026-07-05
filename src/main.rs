#![allow(dead_code)]

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod agents;
mod archive;
mod auth;
mod cli;
mod config;
mod db;
mod logging;
mod omp;
mod orchestration;
mod providers;

use clap::Parser;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

mod server;
mod services;
mod worktree;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Cli::parse();
    if let Some(cmd) = args.command {
        cli::agent::handle_command(cmd).await?;
        return Ok(());
    }

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

    let orphans = orchestration::recovery::recover_orphaned_runs(&client).await?;
    tracing::info!("Orphan recovery: {} agent runs swept to failed", orphans);

    let pkce_store = Arc::new(Mutex::new(HashMap::new()));
    let cookie_key = cookie::Key::generate();

    let auth_layer = auth::AuthLayer::new(&cfg, client.clone()).await?;

    let oidc_provider = if auth_layer.enabled {
        let issuer_url = cfg.oidc_issuer_url.as_ref().unwrap();
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );
        let disc: serde_json::Value = reqwest::get(&discovery_url)
            .await
            .map_err(|e| Box::new(std::io::Error::other(e.to_string())))?
            .json()
            .await?;
        let authorization_endpoint = disc["authorization_endpoint"]
            .as_str()
            .ok_or("missing authorization_endpoint")?
            .to_string();
        let token_endpoint = disc["token_endpoint"]
            .as_str()
            .ok_or("missing token_endpoint")?
            .to_string();
        let revocation_endpoint = disc["revocation_endpoint"].as_str().map(|s| s.to_string());
        let redirect_uri = cfg.oidc_redirect_uri.clone().unwrap_or_else(|| {
            format!(
                "{}/api/auth/callback",
                cfg.base_url
                    .as_deref()
                    .unwrap_or("http://localhost:3183")
                    .trim_end_matches('/')
            )
        });
        Some(server::state::OidcEndpoints {
            authorization_endpoint,
            token_endpoint,
            revocation_endpoint,
            client_id: cfg.oidc_client_id.clone().unwrap_or_default(),
            client_secret: cfg.oidc_client_secret.clone(),
            redirect_uri,
        })
    } else {
        None
    };

    let state = server::state::AppState {
        db: client.clone(),
        default_user_id,
        archive_root: cfg.archive_root.clone(),
        config_root: cfg.config_root.clone(),
        omp_sessions: Arc::new(Mutex::new(HashMap::new())),
        active_sessions: Arc::new(Mutex::new(HashMap::new())),
        oidc_provider,
        pkce_store,
        cookie_key,
    };
    tracing::info!(
        "Auth middleware: {}",
        if auth_layer.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    // Server
    let app = server::router(state, auth_layer);
    let addr = format!("{}:{}", cfg.hostname, cfg.port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
