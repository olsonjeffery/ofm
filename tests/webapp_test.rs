use ofm::auth::AuthLayer;
use ofm::config::OfmConfig;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;
use std::collections::HashMap;
use std::path::Path;
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
async fn test_proxy_router_not_mounted_without_rauthy() {
    let (state, auth_layer, _tmp) = make_state().await;
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/auth/v1/health", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    // With `rauthy_port: None` the `/auth` nest is not mounted, so requests
    // to it must 404 rather than being proxied anywhere.
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_webapp_navbar_shows_connection_status_entry() {
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
    assert!(
        body.contains("ws-status-entry"),
        "should render connection status element"
    );
    assert!(body.contains("mdi-wifi"), "should render wifi icon");
    assert!(
        body.contains("agent-dropdown"),
        "should render agent dropdown container"
    );
    assert!(
        body.contains("0 Agents"),
        "should show 0 Agents when none running"
    );
    assert!(!body.contains("disabled"), "trigger should not be disabled");
}

#[tokio::test]
async fn test_webapp_navbar_shows_running_agent() {
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
        hiqlite::params!(project_id, user_id.to_string(), "Agent Test Proj", "/tmp/test", &now),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "Agent Test Task", "pending", &now),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO conversations (id, task_id, provider_session_id, model, effort, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(conv_id.to_string(), task_id, "sess-1", "gpt-4", "balanced", &now),
    )
    .await
    .unwrap();

    db.execute(
        "INSERT INTO task_agent_runs (id, task_id, agent_type, status, conversation_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(Uuid::new_v4().to_string(), task_id, "implementation", "running", conv_id.to_string(), &now),
    )
    .await
    .unwrap();

    let url = format!("http://{}/webapp", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("ws-status-entry"),
        "should render connection status element"
    );
    assert!(
        body.contains("mdi-message-outline"),
        "should render message icon in button"
    );
    assert!(body.contains("1 Agents"), "should show 1 Agent");
    assert!(
        body.contains(&conv_id.to_string()),
        "should contain conversation UUID in link"
    );
    assert!(
        body.contains("mdi-code-tags"),
        "should render implementation agent icon"
    );
    assert!(
        body.contains("dropdown-divider"),
        "should render dropdown divider"
    );
}

#[tokio::test]
async fn test_agent_status_endpoint_returns_running_question_and_blocked() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let now = chrono::Utc::now().naive_utc().to_string();

    let running_project = int64_id();
    let running_task = int64_id();
    let conv_id = Uuid::new_v4();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(running_project, user_id.to_string(), "Running Proj", "/tmp/running-test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(running_task, running_project, user_id.to_string(), "Running Task", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO conversations (id, task_id, provider_session_id, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        hiqlite::params!(conv_id.to_string(), running_task, "sess-running", "gpt-4", "balanced", &now, &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO task_agent_runs (id, task_id, agent_type, status, conversation_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(Uuid::new_v4().to_string(), running_task, "implementation", "running", conv_id.to_string(), &now),
    )
    .await
    .unwrap();

    let question_project = int64_id();
    let question_task = int64_id();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(question_project, user_id.to_string(), "Question Proj", "/tmp/question-test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(question_task, question_project, user_id.to_string(), "Question Task", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO conversations (id, task_id, provider_session_id, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        hiqlite::params!(Uuid::new_v4().to_string(), question_task, "sess-question", "gpt-4", "balanced", &now, &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO messages (project_key, session_id, seq, entry_json, timestamp) VALUES ($1, $2, 1, $3, $4)",
        hiqlite::params!(question_task, "sess-question",
            serde_json::json!({"type": "question_asked", "session_id": "sess-question", "questions": [{"question": "Pick one", "options": []}], "timestamp": "2024-01-01T00:00:00"}).to_string(),
            &now),
    )
    .await
    .unwrap();

    let blocked_project = int64_id();
    let blocked_task = int64_id();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(blocked_project, user_id.to_string(), "Blocked Proj", "/tmp/blocked-test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, workflow_blocked, created_at) VALUES ($1, $2, $3, $4, $5, 1, $6)",
        hiqlite::params!(blocked_task, blocked_project, user_id.to_string(), "Blocked Task", "pending", &now),
    )
    .await
    .unwrap();

    let url = format!("http://{}/api/tasks/agent-status", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    let running = body["agents"].as_array().unwrap();
    assert_eq!(running.len(), 1, "should list the running agent");
    assert_eq!(running[0]["task_id"], running_task);
    assert_eq!(running[0]["agent_type"], "implementation");

    let questions = body["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 1, "should list the open-question task");
    assert_eq!(questions[0]["task_id"], question_task);

    let blocked = body["blocked"].as_array().unwrap();
    assert_eq!(blocked.len(), 1, "should list the blocked task");
    assert_eq!(blocked[0]["task_id"], blocked_task);
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
async fn test_webapp_system_status_page() {
    let (state, auth_layer, _tmp) = make_state().await;
    let db = state.db.clone();
    // Seed a live row so the page has at least one status icon.
    let entries = vec![ofm::services::system_health::HealthEntry {
        category: "live",
        resource: "live:opencode-pool".into(),
        status: ofm::services::system_health::HealthStatus::Ok,
        detail: "1 pooled opencode server(s)".into(),
        metadata: serde_json::json!({"pid": 1234}),
    }];
    ofm::services::system_health::refresh_entries(&db, &entries)
        .await
        .unwrap();

    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/webapp/system", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("System Status"),
        "page should render the System Status heading"
    );
    assert!(
        body.contains("live:opencode-pool"),
        "seeded live row should render"
    );
    assert!(
        body.contains("data-utc"),
        "timestamps should carry data-utc attributes"
    );
    assert!(
        body.contains("running-services-count"),
        "running services badge should be present"
    );
    assert!(
        body.contains("mdi-heart-pulse"),
        "page should use the heart-pulse icon"
    );
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
        jwks_refresh_url: None,
        client_id: None,
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: user_id,
    };
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
        "/webapp/login?return_to=%2Fwebapp"
    );
}

#[tokio::test]
async fn test_callback_skips_onboarding_when_completed() {
    let (state, auth_layer, _tmp) = make_state_with_webapp_auth().await;
    let key = cookie::Key::generate();
    let state = AppState {
        cfg_port: 0,
        rauthy_port: None,
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
        rauthy_port: None,
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
        rauthy_port: None,

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
    assert!(body.contains("No conversations yet"));
    assert!(body.contains("agent-run-buttons"));
    assert!(
        body.contains("Commits"),
        "task detail page should render the commits section"
    );
    assert!(
        body.contains("No commits yet."),
        "task without a worktree should show the commits empty state"
    );
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

async fn git_run(dir: &Path, args: &[&str]) {
    let out = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .expect("git spawn failed");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

async fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .expect("git spawn failed");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test]
async fn test_webapp_task_detail_page_shows_commits_from_worktree() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Build a repo with a base commit, a simulated origin default branch, and a
    // feature worktree holding one commit on top of the base.
    let repo_dir = TempDir::new().unwrap();
    let repo_path = repo_dir.path().to_string_lossy().to_string();
    git_run(repo_dir.path(), &["init", "--initial-branch=main"]).await;
    git_run(repo_dir.path(), &["config", "user.email", "test@test.com"]).await;
    git_run(repo_dir.path(), &["config", "user.name", "Test"]).await;
    tokio::fs::write(repo_dir.path().join("base.txt"), "base\n")
        .await
        .unwrap();
    git_run(repo_dir.path(), &["add", "base.txt"]).await;
    git_run(repo_dir.path(), &["commit", "-m", "base commit"]).await;
    git_run(
        repo_dir.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .await;
    git_run(
        repo_dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .await;

    let worktree_dir = repo_dir.path().join("wt-task-7");
    git_run(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "task/7-demo",
            worktree_dir.to_string_lossy().as_ref(),
            "main",
        ],
    )
    .await;
    tokio::fs::write(worktree_dir.join("feature.txt"), "feature\n")
        .await
        .unwrap();
    git_run(&worktree_dir, &["add", "feature.txt"]).await;
    git_run(&worktree_dir, &["commit", "-m", "Add feature"]).await;
    let sha = git_stdout(&worktree_dir, &["rev-parse", "HEAD"]).await;
    let short_sha = &sha[..8];

    let project_id = int64_id();
    let task_id = int64_id();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Git Detail", repo_path.clone(), &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "Git Task", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO worktrees (id, project_id, task_id, worktree_path, repo_path, branch, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        hiqlite::params!(
            Uuid::new_v4().to_string(),
            project_id,
            task_id,
            worktree_dir.to_string_lossy().to_string(),
            repo_path,
            "task/7-demo",
            &now
        ),
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
    assert!(body.contains("Git Task"));
    assert!(body.contains("Add feature"), "commit summary should render");
    assert!(
        body.contains(short_sha),
        "commit short oid should render in the commits table"
    );
    assert!(
        body.contains(&format!(
            "/webapp/projects/{}/tasks/{}/commits/{}",
            project_id, task_id, short_sha
        )),
        "commit row should link to the commit detail page"
    );
    assert!(
        !body.contains("No commits yet."),
        "commit list should be populated"
    );
}

#[tokio::test]
async fn test_webapp_commit_detail_page() {
    let (state, auth_layer, _tmp) = make_state().await;
    let user_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let repo_dir = TempDir::new().unwrap();
    let repo_path = repo_dir.path().to_string_lossy().to_string();
    git_run(repo_dir.path(), &["init", "--initial-branch=main"]).await;
    git_run(repo_dir.path(), &["config", "user.email", "test@test.com"]).await;
    git_run(repo_dir.path(), &["config", "user.name", "Test"]).await;
    tokio::fs::write(repo_dir.path().join("a.txt"), "one\n")
        .await
        .unwrap();
    git_run(repo_dir.path(), &["add", "a.txt"]).await;
    git_run(repo_dir.path(), &["commit", "-m", "base"]).await;
    git_run(
        repo_dir.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .await;
    git_run(
        repo_dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .await;

    let worktree_dir = repo_dir.path().join("wt-task-8");
    git_run(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "task/8-demo",
            worktree_dir.to_string_lossy().as_ref(),
            "main",
        ],
    )
    .await;
    tokio::fs::write(worktree_dir.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    git_run(&worktree_dir, &["add", "a.txt"]).await;
    git_run(&worktree_dir, &["commit", "-m", "add second line"]).await;
    let sha = git_stdout(&worktree_dir, &["rev-parse", "HEAD"]).await;
    let short_sha = &sha[..8];

    let project_id = int64_id();
    let task_id = int64_id();
    let now = chrono::Utc::now().naive_utc().to_string();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(project_id, user_id.to_string(), "Diff Detail", repo_path.clone(), &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "Diff Task", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO worktrees (id, project_id, task_id, worktree_path, repo_path, branch, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        hiqlite::params!(
            Uuid::new_v4().to_string(),
            project_id,
            task_id,
            worktree_dir.to_string_lossy().to_string(),
            repo_path,
            "task/8-demo",
            &now
        ),
    )
    .await
    .unwrap();

    let url = format!(
        "http://{}/webapp/projects/{}/tasks/{}/commits/{}",
        addr, project_id, task_id, short_sha
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("add second line"),
        "commit summary should render"
    );
    assert!(body.contains("a.txt"), "changed file path should render");
    assert!(body.contains("diff-grid"), "diff should render");
    assert_eq!(
        body.matches("one\n").count(),
        1,
        "context line should render exactly once (no duplicate rendering)"
    );
    assert!(body.contains("two\n"), "inserted line should render");
    assert!(
        body.contains("diff-add"),
        "inserted line should be tinted green"
    );
}

#[tokio::test]
async fn test_webapp_commit_detail_page_bad_oid_renders_not_found() {
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
        hiqlite::params!(project_id, user_id.to_string(), "No Commit", "/tmp/test", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(task_id, project_id, user_id.to_string(), "No Commit Task", "pending", &now),
    )
    .await
    .unwrap();

    let url = format!(
        "http://{}/webapp/projects/{}/tasks/{}/commits/{}",
        addr, project_id, task_id, "deadbeef"
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("Commit not found."),
        "a bad oid should render the not-found empty state, not error"
    );
}

#[tokio::test]
async fn test_webapp_cross_user_task_access_returns_404() {
    let (state, auth_layer, _tmp) = make_state().await;
    let attacker_id = state.default_user_id;
    let db = state.db.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let repo_dir = TempDir::new().unwrap();
    let repo_path = repo_dir.path().to_string_lossy().to_string();
    git_run(repo_dir.path(), &["init", "--initial-branch=main"]).await;
    git_run(repo_dir.path(), &["config", "user.email", "test@test.com"]).await;
    git_run(repo_dir.path(), &["config", "user.name", "Test"]).await;
    tokio::fs::write(repo_dir.path().join("secret.txt"), "secret\n")
        .await
        .unwrap();
    git_run(repo_dir.path(), &["add", "secret.txt"]).await;
    git_run(repo_dir.path(), &["commit", "-m", "base"]).await;
    git_run(
        repo_dir.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .await;
    git_run(
        repo_dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .await;

    let worktree_dir = repo_dir.path().join("wt-task-9");
    git_run(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "task/9-demo",
            worktree_dir.to_string_lossy().as_ref(),
            "main",
        ],
    )
    .await;
    tokio::fs::write(worktree_dir.join("secret.txt"), "secret\nhidden\n")
        .await
        .unwrap();
    git_run(&worktree_dir, &["add", "secret.txt"]).await;
    git_run(&worktree_dir, &["commit", "-m", "leak me"]).await;
    let sha = git_stdout(&worktree_dir, &["rev-parse", "HEAD"]).await;
    let short_sha = &sha[..8];

    let attacker_project_id = int64_id();
    let victim_project_id = int64_id();
    let victim_task_id = int64_id();
    let victim_user_id = Uuid::new_v4();
    let now = chrono::Utc::now().naive_utc().to_string();

    // The victim owns a project + task with a worktree holding commits.
    let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    db.execute(
        "INSERT INTO users (id, username, is_admin, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, $4, $5, 1, 0)",
        hiqlite::params!(victim_user_id.to_string(), "victim-user", 0, 1, now_str),
    )
    .await
    .unwrap();

    // The authenticated user owns a project, so the project ownership check
    // passes, but the task belongs to a different user/project.
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(attacker_project_id, attacker_id.to_string(), "Attacker Proj", "/tmp/attacker", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(victim_project_id, victim_user_id.to_string(), "Victim Proj", repo_path.clone(), &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(victim_task_id, victim_project_id, victim_user_id.to_string(), "Victim Task", "pending", &now),
    )
    .await
    .unwrap();
    db.execute(
        "INSERT INTO worktrees (id, project_id, task_id, worktree_path, repo_path, branch, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        hiqlite::params!(
            Uuid::new_v4().to_string(),
            victim_project_id,
            victim_task_id,
            worktree_dir.to_string_lossy().to_string(),
            repo_path,
            "task/9-demo",
            &now
        ),
    )
    .await
    .unwrap();

    let client = reqwest::Client::new();
    let task_detail_url = format!(
        "http://{}/webapp/projects/{}/tasks/{}",
        addr, attacker_project_id, victim_task_id
    );
    let resp = client.get(&task_detail_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        404,
        "another user's task must not render on the task detail page"
    );

    let commit_detail_url = format!(
        "http://{}/webapp/projects/{}/tasks/{}/commits/{}",
        addr, attacker_project_id, victim_task_id, short_sha
    );
    let resp = client.get(&commit_detail_url).send().await.unwrap();
    assert_eq!(
        resp.status(),
        404,
        "another user's commit diff must not render"
    );
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
    assert!(body.contains("chat-status-bar"));
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
    assert!(body.contains("chat-status-bar"));
    assert!(body.contains("Agent Idle"));
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
        rauthy_port: None,

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
        "/webapp/login?return_to=%2Fwebapp"
    );
}
