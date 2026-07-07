use omprint::auth::AuthLayer;
use omprint::db;
use omprint::providers::LlmProvider;
use omprint::server;
use omprint::server::state::AppState;
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
    let auth_layer = AuthLayer::disabled(client.clone(), b"test".to_vec(), cookie::Key::generate());
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
async fn test_redirect_root_to_webapp() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/", addr);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 308);
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/webapp"
    );
}

#[tokio::test]
async fn test_webapp_shell_page() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/webapp", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<html"));
    assert!(body.contains("omprint"));
    assert!(body.contains(r#"data-island="uptime""#));
}

#[tokio::test]
async fn test_uptime_island_endpoint() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/webapp/islands/uptime", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Server Uptime"));
}

#[tokio::test]
async fn test_infocard_island_endpoint() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!(
        "http://{}/webapp/islands/infocard?title=Hello&body=World",
        addr
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Hello"));
    assert!(body.contains("World"));
}

#[tokio::test]
async fn test_nonexistent_webapp_route_returns_404() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/webapp/nonexistent", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 404);
}

fn make_encrypted_cookie(key: &cookie::Key, name: &str, value: &str) -> String {
    let mut jar = cookie::CookieJar::new();
    {
        let mut private = jar.private_mut(key);
        private.add(cookie::Cookie::new(name.to_owned(), value.to_owned()));
    }
    let c = jar.get(name).unwrap();
    format!("{}={}", c.name(), c.value())
}

async fn make_state_with_webapp_auth() -> (AppState, AuthLayer, TempDir) {
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
    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(None)),
        issuer_url: None,
        client_id: None,
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
    };
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
async fn test_webapp_protected_route_redirects_without_session() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
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
    let resp = client
        .get(format!("http://{}/webapp", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 302);
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/webapp/login"
    );
}

#[tokio::test]
async fn test_webapp_protected_route_allows_with_valid_session() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cookie_key: key.clone(),
        ..state
    };
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let future = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.execute(
        "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
        hiqlite::params!(user_id.to_string(), "webappuser", "webapp-sub", &now),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(session_id.to_string(), user_id.to_string(), "refresh-token", future, &now),
    )
    .await
    .unwrap();

    let cookie_str = make_encrypted_cookie(&key, "omprint_session", &session_id.to_string());
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/webapp", addr))
        .header("Cookie", cookie_str)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<html"));
    assert!(body.contains("omprint"));
}

#[tokio::test]
async fn test_webapp_protected_route_redirects_with_expired_session() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cookie_key: key.clone(),
        ..state
    };
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let past = (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.execute(
        "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
        hiqlite::params!(user_id.to_string(), "webappuser2", "webapp-sub2", &now),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(session_id.to_string(), user_id.to_string(), "refresh-token", past, &now),
    )
    .await
    .unwrap();

    let cookie_str = make_encrypted_cookie(&key, "omprint_session", &session_id.to_string());
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .get(format!("http://{}/webapp", addr))
        .header("Cookie", cookie_str)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 302);
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/webapp/login"
    );
}
