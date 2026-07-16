use ofm::auth::AuthLayer;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;

use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;
use uuid::Uuid;

struct TestApp {
    addr: String,
    _handle: tokio::task::JoinHandle<()>,
    db: hiqlite::Client,
    project_id: i64,
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

    // Seed a harness config so config checks pass (Phase 3 feature)
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            hiqlite::params!(
                Uuid::new_v4().to_string(),
                "implementation",
                "oh-my-pi",
                "test-config.yaml",
                "global",
                "gpt-4",
                "balanced",
                &now,
                &now
            ),
        )
        .await
        .unwrap();

    let project_id = ofm::services::projects::create_project(
        &client,
        &user_id,
        "test-project",
        "/tmp/test-repo",
        None,
    )
    .await
    .unwrap()
    .id;

    let auth_layer = AuthLayer::disabled(
        client.clone(),
        b"test".to_vec(),
        cookie::Key::generate(),
        user_id,
    );
    let state = AppState {
        cfg_port: 0,

        db: client.clone(),
        default_user_id: user_id,
        footprint: db_dir.path().to_str().unwrap().to_string(),
        archive_root: "storage/".into(),
        config_root: db_dir.path().to_str().unwrap().to_string(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
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

async fn create_task_seed(db: &hiqlite::Client, project_id: i64) -> i64 {
    let user_id = default_user_id(db).await;
    let task = ofm::services::tasks::create_task(db, project_id, &user_id, "test-task", "pending")
        .await
        .unwrap();
    task.id
}

async fn create_task_seed_with_count(db: &hiqlite::Client, project_id: i64, run_count: i32) -> i64 {
    let user_id = default_user_id(db).await;
    let task = ofm::services::tasks::create_task(db, project_id, &user_id, "test-task", "pending")
        .await
        .unwrap();
    // Update run count after creation
    db.execute(
        "UPDATE tasks SET workflow_run_count = $1 WHERE id = $2",
        hiqlite::params!(run_count as i64, task.id),
    )
    .await
    .unwrap();
    task.id
}

#[tokio::test]
async fn test_create_agent_run_201() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, app.project_id).await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "running");
    assert_eq!(body["task_id"].as_i64().unwrap(), task_id);
    assert_eq!(body["agent_type"], "implementation");
    assert!(body["id"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn test_create_agent_run_409_concurrent() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, app.project_id).await;

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
    let fake_id: i64 = 99999;

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
    let task_id = create_task_seed(&app.db, app.project_id).await;

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
    let task_id = create_task_seed_with_count(&app.db, app.project_id, 25).await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn test_stop_agent_runs_marks_running_as_failed() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, app.project_id).await;

    // Create a running agent run
    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .json(&serde_json::json!({ "agent_type": "implementation" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let run_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "running");

    // Call stop endpoint — should sweep and mark all runs as failed
    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs/stop", app.addr, task_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the run is now marked as failed
    let resp = client()
        .get(format!("{}/api/tasks/{}/agent-runs", app.addr, task_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let runs: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["status"], "failed");
    assert_eq!(runs[0]["id"], run_id);
}

#[tokio::test]
async fn test_stop_agent_runs_no_running_runs() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, app.project_id).await;

    // No running runs — stop should be a no-op
    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs/stop", app.addr, task_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_stop_agent_runs_task_not_found() {
    let app = setup_app().await;

    let resp = client()
        .post(format!("{}/api/tasks/{}/agent-runs/stop", app.addr, 99999))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_list_agent_runs() {
    let app = setup_app().await;
    let task_id = create_task_seed(&app.db, app.project_id).await;

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
        ofm::services::tasks::mark_agent_run_failed(&app.db, &run_id)
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
