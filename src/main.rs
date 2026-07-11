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

mod orchestration;
mod providers;
mod rauthy;

use clap::Parser;
use server::ws::bus::BroadcastBus;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

mod server;
mod services;
mod webapp;
mod worktree;

fn box_io_err(e: std::io::Error) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::other(e.to_string()))
}

fn ensure_secret_file(
    path: &std::path::Path,
    data: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(box_io_err)?;
    }
    std::fs::write(path, data).map_err(box_io_err)?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

type OidcDiscoveryResult = (String, String, Option<String>, Option<String>);

fn parse_oidc_discovery(
    disc: &serde_json::Value,
) -> Result<OidcDiscoveryResult, Box<dyn std::error::Error>> {
    Ok((
        disc["authorization_endpoint"]
            .as_str()
            .ok_or("missing authorization_endpoint")?
            .to_string(),
        disc["token_endpoint"]
            .as_str()
            .ok_or("missing token_endpoint")?
            .to_string(),
        disc["revocation_endpoint"].as_str().map(|s| s.to_string()),
        disc["end_session_endpoint"].as_str().map(|s| s.to_string()),
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Cli::parse();
    if let Some(cmd) = args.command {
        cli::agent::handle_command(cmd).await?;
        return Ok(());
    }

    let cfg = config::OfmConfig::load();

    let logging_config = cfg
        .logging_config_path
        .as_ref()
        .map(|p| std::path::PathBuf::from(p));
    logging::init_with_config(logging_config.as_ref());

    // DB setup
    std::fs::create_dir_all(&cfg.data_dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(&cfg.data_dir, std::fs::Permissions::from_mode(0o700))?;

    // If the hiqlite addresses have changed since the last run (e.g. upgrading
    // from a version that used port 0), reset the Raft logs so the cluster
    // membership is re-initialized from NodeConfig rather than stale Raft state.
    let addr_fingerprint = format!("{}:{}", cfg.hiqlite_raft_port, cfg.hiqlite_api_port);
    let fingerprint_path = std::path::Path::new(&cfg.data_dir).join(".addr_fingerprint");
    let needs_raft_reset = if fingerprint_path.exists() {
        let prev = std::fs::read_to_string(&fingerprint_path).unwrap_or_default();
        prev.trim() != addr_fingerprint
    } else {
        // No fingerprint at all → either fresh install or pre-fingerprint version.
        // If Raft logs exist, they may be stale (e.g. from port-0 era).
        std::path::Path::new(&cfg.data_dir).join("logs").exists()
    };
    if needs_raft_reset {
        let logs_dir = std::path::Path::new(&cfg.data_dir).join("logs");
        if logs_dir.exists() {
            tracing::info!("hiqlite address config changed — resetting Raft logs to re-initialise cluster membership");
            std::fs::remove_dir_all(&logs_dir)?;
        }
    }
    std::fs::write(&fingerprint_path, &addr_fingerprint)?;

    let node_config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: format!("127.0.0.1:{}", cfg.hiqlite_raft_port),
            addr_api: format!("127.0.0.1:{}", cfg.hiqlite_api_port),
        }],
        data_dir: cfg.data_dir.clone().into(),
        secret_raft: std::env::var("OFM_RAFT_SECRET").unwrap_or_else(|_| {
            tracing::warn!("OFM_RAFT_SECRET not set — using insecure default");
            "ofm-raft-secret-0123456".into()
        }),
        secret_api: std::env::var("OFM_API_SECRET").unwrap_or_else(|_| {
            tracing::warn!("OFM_API_SECRET not set — using insecure default");
            "omprint-api-secret-0123456".into()
        }),
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

    let cookie_key_path = std::path::Path::new(&cfg.config_root).join("cookie_key.bin");
    let cookie_key = if cookie_key_path.exists() {
        let data = std::fs::read(&cookie_key_path).map_err(box_io_err)?;
        cookie::Key::from(&data)
    } else {
        let key = cookie::Key::generate();
        let mut combined = vec![0u8; 64];
        combined[..32].copy_from_slice(key.signing());
        combined[32..64].copy_from_slice(key.encryption());
        ensure_secret_file(&cookie_key_path, &combined)?;
        key
    };

    let api_key_pepper_path = std::path::Path::new(&cfg.config_root).join("api_key_pepper.bin");
    let api_key_pepper = if api_key_pepper_path.exists() {
        std::fs::read(&api_key_pepper_path).map_err(box_io_err)?
    } else {
        let pepper: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
        ensure_secret_file(&api_key_pepper_path, &pepper)?;
        pepper
    };

    let mut _rauthy_instance: Option<rauthy::RauthyInstance> = None;

    let (auth_layer, oidc_provider) = if cfg.rauthy_enabled {
        let rp = if cfg.rauthy_port > 0 {
            cfg.rauthy_port
        } else {
            rauthy::find_available_port()?
        };

        tracing::info!("Starting embedded rauthy on port {}", rp);
        let instance = rauthy::start_rauthy(&cfg.footprint, rp, cfg.port).await?;
        rauthy::wait_until_healthy(rp).await?;
        tracing::info!("rauthy is healthy");
        _rauthy_instance = Some(instance);

        let direct_base = format!("http://127.0.0.1:{}", rp);
        let discovery_url = format!("{}/.well-known/openid-configuration", direct_base);
        let disc: serde_json::Value = reqwest::get(&discovery_url).await?.json().await?;
        let issuer = disc["issuer"].as_str().ok_or("missing issuer")?.to_string();
        let (authorization_endpoint, token_endpoint, revocation_endpoint, end_session_endpoint) =
            parse_oidc_discovery(&disc)?;
        let redirect_uri = format!("http://127.0.0.1:{}/api/auth/callback", cfg.port);
        let client_id = cfg.oidc_client_id.clone().unwrap_or_else(|| "ofm".into());

        let jwks_disc_url = format!(
            "http://127.0.0.1:{}/auth/v1/.well-known/openid-configuration",
            rp
        );
        let jwks_disc: serde_json::Value = reqwest::get(&jwks_disc_url).await?.json().await?;
        let jwks_uri = jwks_disc["jwks_uri"]
            .as_str()
            .ok_or("missing jwks_uri")?
            .to_string();
        let jwks_resp: serde_json::Value = reqwest::get(&jwks_uri).await?.json().await?;
        let keys: std::collections::HashMap<String, jsonwebtoken::jwk::Jwk> = jwks_resp["keys"]
            .as_array()
            .ok_or("missing keys array")?
            .iter()
            .filter_map(|k| {
                let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(k.clone()).ok()?;
                jwk.common.key_id.clone().map(|kid| (kid, jwk))
            })
            .collect();

        let jwks_cache = auth::jwks::JwksCache {
            keys,
            issuer: issuer.clone(),
            client_id: client_id.clone(),
        };
        let auth_layer_rauthy = auth::AuthLayer::from_cache(
            jwks_cache,
            client.clone(),
            api_key_pepper.clone(),
            cookie_key.clone(),
            default_user_id,
        );
        let oidc_endpoints = server::state::OidcEndpoints {
            authorization_endpoint,
            token_endpoint,
            end_session_endpoint,
            revocation_endpoint,
            client_id,
            client_secret: cfg.oidc_client_secret.clone(),
            redirect_uri,
            jwks_cache: Some(auth_layer_rauthy.jwks_cache.clone()),
            jwks_issuer: auth_layer_rauthy.issuer_url.clone(),
        };
        (auth_layer_rauthy, Some(oidc_endpoints))
    } else {
        let auth_layer = auth::AuthLayer::new(
            &cfg,
            client.clone(),
            api_key_pepper.clone(),
            cookie_key.clone(),
            default_user_id,
        )
        .await
        .unwrap_or_else(|e| {
            eprintln!("ERROR: Failed to initialize OIDC authentication: {e}");
            std::process::exit(1);
        });

        if !auth_layer.enabled {
            eprintln!("ERROR: OIDC is not configured. Set OFM_OIDC_ISSUER_URL (and OFM_OIDC_CLIENT_ID) to enable authentication.");
            std::process::exit(1);
        }

        let issuer_url = cfg.oidc_issuer_url.as_ref().unwrap();
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );
        let disc: serde_json::Value = reqwest::get(&discovery_url).await?.json().await?;
        let (authorization_endpoint, token_endpoint, revocation_endpoint, end_session_endpoint) =
            parse_oidc_discovery(&disc)?;
        let redirect_uri = cfg.oidc_redirect_uri.clone().unwrap_or_else(|| {
            format!(
                "{}/api/auth/callback",
                cfg.base_url
                    .as_deref()
                    .unwrap_or("http://localhost:3183")
                    .trim_end_matches('/')
            )
        });
        let oidc_provider = Some(server::state::OidcEndpoints {
            authorization_endpoint,
            token_endpoint,
            end_session_endpoint,
            revocation_endpoint,
            client_id: cfg.oidc_client_id.clone().unwrap_or_default(),
            client_secret: cfg.oidc_client_secret.clone(),
            redirect_uri,
            jwks_cache: Some(auth_layer.jwks_cache.clone()),
            jwks_issuer: auth_layer.issuer_url.clone(),
        });

        (auth_layer, oidc_provider)
    };

    let state = server::state::AppState {
        db: client.clone(),
        default_user_id,
        footprint: cfg.footprint.clone(),
        archive_root: cfg.archive_root.clone(),
        config_root: cfg.config_root.clone(),

        active_sessions: Arc::new(Mutex::new(HashMap::new())),
        oidc_provider,
        pkce_store,
        cookie_key,
        api_key_pepper,
        cfg_port: cfg.port,
        ws_bus: BroadcastBus::new(),
    };
    tracing::info!("Auth middleware: enabled");

    // Server
    let app = server::router(state, auth_layer);
    let addr = format!("{}:{}", cfg.hostname, cfg.port);
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
