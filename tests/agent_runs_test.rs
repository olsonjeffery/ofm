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

struct TestApp {
    addr: String,
    _handle: tokio::task::JoinHandle<()>,
    db: hiqlite::Client,
    project_id: Uuid,
    _db_dir: TempDir,
}

async fn setup_app() -> TestApp {
    let db_dir = TempDir::new().unwrap();
    let config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: "127.0.0.1:0".into(),
            addr_api: "127.0.0.1:0".into(),
        }],
        data_dir: db_dir.path().to_str().unwrap().to_string().into(),
        secret_raft: "test-raft-secret-123".into(),
        secret_api: "test-api-secret-123".into(),
        ..Default::default()
    };
    let client = hiqlite::start_node(config).await.unwrap();
    client.wait_until_healthy_db().await;
    db::run_migrations(&client).await.unwrap();
    let user_id = db::ensure_default_user(&client).await.unwrap();

    let project_id = omprint::services::projects::create_project(
        &client,
        &user_id,
        "test-project",
        "/tmp/test-repo",
        None,
    )
    .await
    .unwrap()
    .id;

    let auth_layer = AuthLayer::disabled(client.clone(), b"test".to_vec(), cookie::Key::generate());
    let state = AppState {
        cfg_port: 0,

        db: client.clone(),
        default_user_id: user_id,
        archive_root: "storage/".into(),
        config_root: db_dir.path().to_str().unwrap().to_string(),
        omp_sessions: Arc::new(Mutex::new(HashMap::new())),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
    };

    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestApp {
        addr,
        _handle: handle,
        db: client,
        project_id,
        _db_dir: db_dir,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn default_user_id(db: &hiqlite::Client) -> Uuid {
    let mut rows = db
        .query_raw(
            "SELECT id FROM users WHERE username = 'default'",
            hiqlite::params!(),
        )
        .await
        .unwrap();
    let id_str: String = rows[0].get("id");
    Uuid::parse_str(&id_str).unwrap()
}

async fn create_task_seed(db: &hiqlite::Client, project_id: &Uuid) -> Uuid {
    let user_id = default_user_id(db).await;
    let task_id = Uuid::new_v4();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status) VALUES ($1, $2, $3, $4, $5)",
        hiqlite::params!(
            task_id.to_string(),
            project_id.to_string(),
            user_id.to_string(),
            "test-task",
            "pending"
        ),
    )
    .await
    .unwrap();
    task_id
}

async fn create_task_seed_with_count(
    db: &hiqlite::Client,
    project_id: &Uuid,
    run_count: i32,
) -> Uuid {
    let user_id = default_user_id(db).await;
    let task_id = Uuid::new_v4();
    db.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status, workflow_run_count) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(
            task_id.to_string(),
            project_id.to_string(),
            user_id.to_string(),
            "test-task",
            "pending",
            run_count as i64
        ),
    )
    .await
    .unwrap();
    task_id
}

#[tokio::test]
async fn test_create_agent_run_201() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, &app.project_id).await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "running");
    assert_eq!(body["task_id"].as_str().unwrap(), task_id.to_string());
    assert_eq!(body["agent_type"], "implementation");
    assert!(body["id"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn test_create_agent_run_409_concurrent() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, &app.project_id).await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn test_create_agent_run_404_task_not_found() {
    let app = setup_app().await;
    let fake_id = Uuid::new_v4();

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, fake_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_create_agent_run_400_invalid_agent_type() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, &app.project_id).await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "invalid" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_create_agent_run_409_iteration_cap() {
    let app = setup_app().await;
    let task_id = create_task_seed_with_count(&app.db, &app.project_id, 25).await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn test_list_agent_runs() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, &app.project_id).await;

    for _ in 0..3 {
        let resp = client()
            .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
            .json(&serde_json::json!({ "agent_type": "implementation" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);

        let body: serde_json::Value = resp.json().await.unwrap();
        let run_id = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();
        omprint::services::tasks::mark_agent_run_failed(&app.db, &run_id)
            .await
            .unwrap();
    }

    let resp = client()
        .get(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 3);
}
