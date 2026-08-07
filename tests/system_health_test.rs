use ofm::auth::{AuthLayer, AuthUser};
use ofm::config::OfmConfig;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;
use ofm::services;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;
use uuid::Uuid;

async fn make_state() -> (AppState, AuthLayer, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: "127.0.0.1:0".into(),
            addr_api: "127.0.0.1:0".into(),
        }],
        data_dir: tmp.path().to_str().unwrap().to_string().into(),
        secret_raft: "test-raft-secret-123".into(),
        secret_api: "test-api-secret-123".into(),
        ..Default::default()
    };
    let client = hiqlite::start_node(config).await.unwrap();
    client.wait_until_healthy_db().await;
    db::run_migrations(&client).await.unwrap();
    let user_id = db::ensure_default_user(&client).await.unwrap();
    db::ensure_admins_group(&client).await.unwrap();
    let auth_layer = AuthLayer::disabled(
        client.clone(),
        b"test".to_vec(),
        cookie::Key::generate(),
        user_id,
    );
    let state = AppState {
        cfg_port: 0,
        rauthy_port: None,
        db: client,
        default_user_id: user_id,
        footprint: tmp.path().to_str().unwrap().to_string(),
        archive_root: "storage/".into(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
        config: OfmConfig::default(),
    };
    (state, auth_layer, tmp)
}

async fn spawn_server(state: AppState, auth_layer: AuthLayer) -> std::net::SocketAddr {
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn test_system_status_endpoint_shape() {
    let (state, auth_layer, _tmp) = make_state().await;
    let addr = spawn_server(state.clone(), auth_layer).await;

    // Seed a couple of rows so the report is non-empty.
    let cfg = OfmConfig::default();
    let entries = services::system_health::live_health_check(&state.db, &cfg).await;
    services::system_health::refresh_entries(&state.db, &entries)
        .await
        .unwrap();

    let url = format!("http://{}/api/system/status", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["generated_at"].is_string());
    assert!(body["running_services"].is_i64());
    let entries = body["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "seeded live rows should be reported");
    for e in entries {
        assert!(e["category"].is_string());
        assert!(e["resource"].is_string());
        assert!(e["status"].is_string());
        assert!(e["detail"].is_string());
        assert!(e["created_at"].is_string());
    }
}

#[tokio::test]
async fn test_system_history_endpoint_limits_rows() {
    let (state, auth_layer, _tmp) = make_state().await;
    let addr = spawn_server(state.clone(), auth_layer).await;

    // Insert several rows for one resource.
    for _ in 0..7 {
        let entries = vec![services::system_health::HealthEntry {
            category: "dependency",
            resource: "bin:git".into(),
            status: services::system_health::HealthStatus::Ok,
            detail: "test row".into(),
            metadata: serde_json::json!({}),
        }];
        services::system_health::refresh_entries(&state.db, &entries)
            .await
            .unwrap();
    }

    let url = format!(
        "http://{}/api/system/history?resource={}&limit=5",
        addr, "bin%3Agit"
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let rows = body["entries"].as_array().unwrap();
    assert!(rows.len() <= 5, "limit must cap history rows");
    assert!(!rows.is_empty());
    for r in rows {
        assert_eq!(r["resource"], "bin:git");
    }
}

#[tokio::test]
async fn test_refresh_entries_persists_and_prunes() {
    let (state, _auth, _tmp) = make_state().await;
    let db = state.db.clone();

    let mut entries = Vec::new();
    for i in 0..3 {
        entries.push(services::system_health::HealthEntry {
            category: "dependency",
            resource: format!("bin:tool-{i}"),
            status: services::system_health::HealthStatus::Ok,
            detail: format!("detail {i}"),
            metadata: serde_json::json!({"version": "1.0"}),
        });
    }
    let n = services::system_health::refresh_entries(&db, &entries)
        .await
        .unwrap();
    assert_eq!(n, 3);

    let mut rows = db
        .query_raw(
            "SELECT COUNT(*) AS cnt FROM system_health_entry",
            hiqlite::params!(),
        )
        .await
        .unwrap();
    let total: i64 = rows.first_mut().unwrap().get("cnt");
    assert_eq!(total, 3);

    // Prune keeps newest MAX_ROWS_PER_PRUNE — write more than the cap.
    let cap = services::system_health::MAX_ROWS_PER_PRUNE;
    for _ in 0..(cap + 5) {
        services::system_health::refresh_entries(
            &db,
            &[services::system_health::HealthEntry {
                category: "dependency",
                resource: "bin:churn".into(),
                status: services::system_health::HealthStatus::Ok,
                detail: "churn".into(),
                metadata: serde_json::json!({}),
            }],
        )
        .await
        .unwrap();
    }
    let mut rows = db
        .query_raw(
            "SELECT COUNT(*) AS cnt FROM system_health_entry",
            hiqlite::params!(),
        )
        .await
        .unwrap();
    let total: i64 = rows.first_mut().unwrap().get("cnt");
    assert_eq!(total, cap, "rolling log must be pruned to the cap");
}

#[tokio::test]
async fn test_user_can_use_system_health_capability_integration() {
    let (state, _auth, _tmp) = make_state().await;
    let db = state.db.clone();

    let admin_id = db::ensure_default_user(&db).await.unwrap();
    let scoped_id = Uuid::new_v4();
    let plain_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    db.execute(
        "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 0, 1, $3)",
        hiqlite::params!(scoped_id.to_string(), "scoped-u", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 0, 1, $3)",
        hiqlite::params!(plain_id.to_string(), "plain-u", &now),
    )
    .await
    .unwrap();
    db.execute(
        "UPDATE users SET scopes = 'system-status' WHERE id = $1",
        hiqlite::params!(scoped_id.to_string()),
    )
    .await
    .unwrap();

    db::ensure_system_status_group(&db).await.unwrap();

    let auth = |user_id: Uuid, is_admin: bool| AuthUser {
        user_id,
        username: "u".into(),
        oidc_subject: None,
        is_admin,
        is_technical: false,
    };

    assert!(
        services::system_health::user_can_use_system_health(&db, &auth(admin_id, true))
            .await
            .unwrap(),
        "admin holds the capability implicitly"
    );
    assert!(
        services::system_health::user_can_use_system_health(&db, &auth(scoped_id, false))
            .await
            .unwrap(),
        "scoped user holds the capability"
    );
    assert!(
        !services::system_health::user_can_use_system_health(&db, &auth(plain_id, false))
            .await
            .unwrap(),
        "plain user does not hold the capability"
    );
}
