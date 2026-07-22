use ofm::auth::AuthLayer;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;
use uuid::Uuid;

fn int64_id() -> i64 {
    static NEXT_ID: AtomicI64 = AtomicI64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

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
    let auth_layer = AuthLayer::disabled(
        client.clone(),
        b"test".to_vec(),
        cookie::Key::generate(),
        user_id,
    );
    let state = AppState {
        cfg_port: 0,

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
async fn test_webapp_dashboard_page() {
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
    assert!(body.contains("ofm"));
    assert!(body.contains("Projects"));
    assert!(body.contains("New Project"));
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
        default_user_id: user_id,
    };
    let state = AppState {
        cfg_port: 0,

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
async fn test_callback_skips_onboarding_when_completed() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cfg_port: 0,
        cookie_key: key.clone(),
        ..state
    };
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let future = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.execute(
        "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical, git_name, git_email) VALUES ($1, $2, $3, 1, $4, 1, 0, $5, $6)",
        hiqlite::params!(user_id.to_string(), "doneuser", "done-sub", &now, "Jane Doe", "jane@example.com"),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(session_id.to_string(), user_id.to_string(), "refresh-token", future, &now),
    )
    .await
    .unwrap();

    let cookie_str = make_encrypted_cookie(&key, "ofm_session", &session_id.to_string());
    let resp = reqwest::Client::new()
        .get(format!("http://{}/webapp/callback", addr))
        .header("Cookie", cookie_str)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("window.location.href='/webapp/'"),
        "expected redirect to /webapp/, got: {body}"
    );
}

#[tokio::test]
async fn test_callback_routes_to_onboarding_when_not_completed() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cfg_port: 0,
        cookie_key: key.clone(),
        ..state
    };
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let user_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let future = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.execute(
        "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 0, 0)",
        hiqlite::params!(user_id.to_string(), "newuser", "new-sub", &now),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(session_id.to_string(), user_id.to_string(), "refresh-token", future, &now),
    )
    .await
    .unwrap();

    let cookie_str = make_encrypted_cookie(&key, "ofm_session", &session_id.to_string());
    let resp = reqwest::Client::new()
        .get(format!("http://{}/webapp/callback", addr))
        .header("Cookie", cookie_str)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("window.location.href='/webapp/onboarding'"),
        "expected redirect to /webapp/onboarding, got: {body}"
    );
}

#[tokio::test]
async fn test_webapp_protected_route_allows_with_valid_session() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cfg_port: 0,

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

    let cookie_str = make_encrypted_cookie(&key, "ofm_session", &session_id.to_string());
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
    assert!(body.contains("ofm"));
    assert!(body.contains("New Project"));
}

#[tokio::test]
async fn test_webapp_board_page() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let project_id = int64_id();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Board Test Project", "/tmp/test", &now),
    )
    .await
    .unwrap();

    let url = format!("http://{}/webapp/projects/{}", addr, project_id);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Board Test Project"));
    assert!(body.contains("Pending"));
    assert!(body.contains("In Progress"));
    assert!(body.contains("In Review"));
    assert!(body.contains("Completed"));
    assert!(body.contains("New Task"));
}

#[tokio::test]
async fn test_webapp_board_page_404() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/webapp/projects/{}", addr, 99999);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_webapp_task_detail_page() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let project_id = int64_id();
    let task_id = int64_id();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Detail Test", "/tmp/test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "My Test Task", "pending", &now),
    )
    .await
    .unwrap();

    let url = format!(
        "http://{}/webapp/projects/{}/tasks/{}",
        addr, project_id, task_id
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("My Test Task"));
    assert!(body.contains("No document yet"));
    assert!(body.contains("No runs yet"));
    assert!(body.contains("is-info is-light"));
    assert!(body.contains("has-text-info"));
}

#[tokio::test]
async fn test_webapp_task_detail_page_404() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/webapp/projects/{}/tasks/{}", addr, 99999, 99999);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_webapp_chat_page_no_conversations_renders_empty() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let project_id = int64_id();
    let task_id = int64_id();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Chat Test", "/tmp/test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "Chat Task", "pending", &now),
    )
    .await
    .unwrap();

    let url = format!(
        "http://{}/webapp/projects/{}/tasks/{}/chat",
        addr, project_id, task_id
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Chat Task"));
    assert!(body.contains("Chat"));
    assert!(!body.contains("Conversations"), "sidebar should not appear");
    assert!(body.contains("chat-footer"));
}

#[tokio::test]
async fn test_webapp_chat_page_with_conversation_url() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let project_id = int64_id();
    let task_id = int64_id();
    let conv_id = Uuid::new_v4();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Chat Conv Test", "/tmp/test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "Chat Task With Conv", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO conversations (id, task_id, provider_session_id, model, effort, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(conv_id.to_string(), task_id, "sess-1", "gpt-4", "balanced", &now),
    )
    .await
    .unwrap();

    let url = format!(
        "http://{}/webapp/projects/{}/tasks/{}/chat/{}",
        addr, project_id, task_id, conv_id
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Chat Task With Conv"));
    assert!(body.contains("chat-footer"));
    assert!(body.contains(&conv_id.to_string()));
    assert!(
        !body.contains("is-one-quarter"),
        "sidebar should be removed"
    );
}

#[tokio::test]
async fn test_webapp_chat_redirects_to_conversation_when_exists() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let project_id = int64_id();
    let task_id = int64_id();
    let conv_id = Uuid::new_v4();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Chat Redirect Test", "/tmp/test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "Redirect Task", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO conversations (id, task_id, provider_session_id, model, effort, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(conv_id.to_string(), task_id, "sess-1", "gpt-4", "balanced", &now),
    )
    .await
    .unwrap();

    let url = format!(
        "http://{}/webapp/projects/{}/tasks/{}/chat",
        addr, project_id, task_id
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    let expected_url = format!(
        "/webapp/projects/{}/tasks/{}/chat/{}",
        project_id, task_id, conv_id
    );
    assert!(
        body.contains(&expected_url),
        "expected redirect to conversation URL, got body containing: {body}"
    );
}

#[tokio::test]
async fn test_webapp_protected_route_redirects_with_expired_session() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cfg_port: 0,

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

    let cookie_str = make_encrypted_cookie(&key, "ofm_session", &session_id.to_string());
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
