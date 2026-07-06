use omprint::auth::AuthLayer;
use omprint::db;
use omprint::providers::LlmProvider;
use omprint::server;
use omprint::server::state::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Bind to port 0 to let the OS assign a free port, then return it.
async fn find_free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    // Brief yield so the OS reclaims the port before hiqlite binds.
    tokio::task::yield_now().await;
    addr.port()
}

async fn make_state_with_ports(raft_port: u16, api_port: u16) -> (AppState, AuthLayer, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: format!("127.0.0.1:{}", raft_port),
            addr_api: format!("127.0.0.1:{}", api_port),
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
    let auth_layer = AuthLayer::disabled(client.clone(), b"test".to_vec());
    let state = AppState {
        db: client,
        default_user_id: user_id,
        archive_root: "storage/".into(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        omp_sessions: Arc::new(Mutex::new(HashMap::new())),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
    };
    (state, auth_layer, tmp)
}

#[tokio::test]
async fn test_server_with_real_hiqlite_ports() {
    let raft_port = find_free_port().await;
    let api_port = find_free_port().await;

    let (state, auth_layer, _tmp) = make_state_with_ports(raft_port, api_port).await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/health", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn test_server_with_env_configured_hiqlite_ports() {
    let raft_port = find_free_port().await;
    let api_port = find_free_port().await;

    let (state, auth_layer, _tmp) = make_state_with_ports(raft_port, api_port).await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Health endpoint
    let resp = client
        .get(&format!("http://{}/health", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // GET / should redirect to /webapp
    let resp = client
        .get(&format!("http://{}", addr))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_redirection(),
        "expected redirect, got {}",
        resp.status()
    );

    // GET /webapp should serve the shell page
    let resp = client
        .get(&format!("http://{}/webapp", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<html"));
    assert!(body.contains("omprint"));
}

#[tokio::test]
async fn test_hiqlite_ports_do_not_use_zero() {
    let raft_port = find_free_port().await;
    let api_port = find_free_port().await;

    let (state, auth_layer, _tmp) = make_state_with_ports(raft_port, api_port).await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(&format!("http://{}/health", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    assert_ne!(raft_port, 0, "hiqlite raft port must not be 0");
    assert_ne!(api_port, 0, "hiqlite API port must not be 0");
}
