use std::collections::HashMap;
use std::sync::Arc;

use ofm::auth::api_key;
use ofm::auth::AuthLayer;
use ofm::config::OfmConfig;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;
use tokio::sync::Mutex;

fn make_api_key() -> (String, String) {
    let key = "ccui_import_export_test_key_v1";
    let hash = api_key::hash_api_key(key, b"test_pepper_16");
    (key.to_string(), hash)
}

async fn make_state_with_auth() -> (AppState, AuthLayer, String, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().unwrap();
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

    let (api_key_str, hash) = make_api_key();
    client
        .execute(
            "UPDATE users SET api_key_hash = $1 WHERE id = $2",
            hiqlite::params!(hash, user_id.to_string()),
        )
        .await
        .unwrap();

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(None)),
        issuer_url: None,
        jwks_refresh_url: None,
        client_id: None,
        pepper: b"test_pepper_16".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: user_id,
    };

    let state = AppState {
        cfg_port: 0,
        rauthy_port: None,
        db: client,
        default_user_id: user_id,
        footprint: tmp.path().to_str().unwrap().to_string(),
        archive_root: tmp.path().to_str().unwrap().to_string(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
        config: OfmConfig::default(),
    };

    (state, auth_layer, api_key_str, tmp)
}

async fn spawn_app(state: AppState, auth_layer: AuthLayer) -> String {
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

/// Seed a project with tasks and write task doc files to the archive
async fn seed_project(
    client: &hiqlite::Client,
    user_id: &uuid::Uuid,
    archive_root: &str,
    name: &str,
    task_names: &[&str],
) -> i64 {
    let project_id: i64 = {
        let mut rows = client
            .query_raw(
                "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM projects",
                hiqlite::params!(),
            )
            .await
            .unwrap();
        rows.first_mut()
            .map(|r| r.get::<i64>("next_id"))
            .unwrap_or(1)
    };

    let now = || chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
            hiqlite::params!(
                project_id,
                user_id.to_string(),
                name.to_string(),
                format!("/tmp/{name}"),
                now()
            ),
        )
        .await
        .unwrap();

    for task_name in task_names {
        let task_id: i64 = {
            let mut rows = client
                .query_raw(
                    "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM tasks",
                    hiqlite::params!(),
                )
                .await
                .unwrap();
            rows.first_mut()
                .map(|r| r.get::<i64>("next_id"))
                .unwrap_or(1)
        };

        client
            .execute(
                "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(
                    task_id,
                    project_id,
                    user_id.to_string(),
                    task_name.to_string(),
                    "pending".to_string(),
                    now()
                ),
            )
            .await
            .unwrap();

        // Write task doc
        let doc_path = std::path::Path::new(archive_root)
            .join("projects")
            .join(project_id.to_string())
            .join("tasks")
            .join(format!("task-{task_id}.md"));
        std::fs::create_dir_all(doc_path.parent().unwrap()).unwrap();
        let desc = format!("Description for {task_name}");
        std::fs::write(&doc_path, &desc).unwrap();
    }

    project_id
}

fn make_export_json(project_id: i64) -> String {
    serde_json::json!({
        "exported_at": "2026-07-26T01:26:49.727Z",
        "exported_by": { "id": 1, "username": "jeff" },
        "projects": [
            {
                "id": project_id,
                "name": "Test Project",
                "repo_folder_path": "/tmp/test-project",
                "created_at": "2026-07-09 17:40:57",
                "tasks": [
                    {
                        "id": 100,
                        "title": "Test task 1",
                        "status": "pending",
                        "description": "Task description 1",
                        "created_at": "2026-07-09 18:00:00",
                        "conversations": [
                            {
                                "id": "550e8400-e29b-41d4-a716-446655440000",
                                "provider_session_id": "sess-001",
                                "model": "gpt-4",
                                "effort": "high",
                                "name": "Initial chat",
                                "created_at": "2026-07-09 18:00:00",
                                "updated_at": "2026-07-09 18:30:00"
                            }
                        ]
                    }
                ]
            }
        ]
    })
    .to_string()
}

fn make_export_json_with_unknown_fields(project_id: i64) -> String {
    serde_json::json!({
        "exported_at": "2026-07-26T01:26:49.727Z",
        "exported_by": { "id": 1, "username": "jeff" },
        "unknown_top_level": "should be ignored",
        "projects": [
            {
                "id": project_id,
                "name": "Test Project",
                "repo_folder_path": "/tmp/test-project",
                "created_at": "2026-07-09 17:40:57",
                "completed_at": "ignored field",
                "updated_at": "ignored field",
                "tasks": [
                    {
                        "id": 100,
                        "title": "Test task 1",
                        "status": "pending",
                        "description": "Task description 1",
                        "created_at": "2026-07-09 18:00:00",
                        "completed_at": "ignored",
                        "updated_at": "ignored",
                        "conversations": [
                            {
                                "id": "550e8400-e29b-41d4-a716-446655440000",
                                "provider_session_id": "sess-001",
                                "model": "gpt-4",
                                "effort": "high",
                                "name": "Initial chat",
                                "created_at": "2026-07-09 18:00:00",
                                "updated_at": "2026-07-09 18:30:00",
                                "provider": "opencode-go"
                            }
                        ]
                    }
                ]
            }
        ]
    })
    .to_string()
}

// ─── Export Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_export_one_project() {
    let (state, auth_layer, api_key, tmp) = make_state_with_auth().await;
    let db = state.db.clone();
    let user_id = state.default_user_id;
    let archive_root = tmp.path().to_str().unwrap().to_string();
    let base_url = spawn_app(state, auth_layer).await;

    let project_id = seed_project(
        &db,
        &user_id,
        &archive_root,
        "export-test",
        &["Task A", "Task B"],
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "{base_url}/api/settings/export?project_ids={project_id}"
        ))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    assert!(
        body["exported_at"].is_string(),
        "exported_at should be a string"
    );
    assert!(
        body["exported_by"]["id"].is_string(),
        "exported_by.id should be a string"
    );
    assert!(body["exported_by"]["username"].is_string());

    let projects = body["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "should have exactly 1 project");
    assert_eq!(projects[0]["id"], project_id);
    assert_eq!(projects[0]["name"], "export-test");

    let tasks = projects[0]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2, "should have 2 tasks");

    // Verify descriptions are populated from doc files
    let task_a = tasks.iter().find(|t| t["title"] == "Task A").unwrap();
    assert_eq!(task_a["description"], "Description for Task A");
    let task_b = tasks.iter().find(|t| t["title"] == "Task B").unwrap();
    assert_eq!(task_b["description"], "Description for Task B");
}

#[tokio::test]
async fn test_export_selects_specific_projects() {
    let (state, auth_layer, api_key, tmp) = make_state_with_auth().await;
    let db = state.db.clone();
    let user_id = state.default_user_id;
    let archive_root = tmp.path().to_str().unwrap().to_string();
    let base_url = spawn_app(state, auth_layer).await;

    let p1 = seed_project(&db, &user_id, &archive_root, "proj-1", &["T1"]).await;
    let _p2 = seed_project(&db, &user_id, &archive_root, "proj-2", &["T2"]).await;

    let client = reqwest::Client::new();
    // Only export p1
    let resp = client
        .get(format!("{base_url}/api/settings/export?project_ids={p1}"))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let projects = body["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["name"], "proj-1");
}

#[tokio::test]
async fn test_export_requires_auth() {
    let (state, auth_layer, _api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base_url}/api/settings/export?project_ids=1"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ─── Import Preview Tests ───────────────────────────────────────────────────

#[tokio::test]
async fn test_import_preview_valid_json() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let json = make_export_json(42);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/settings/import/preview"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let projects = body["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["source_project_id"], "42");
    assert_eq!(projects[0]["name"], "Test Project");
    assert_eq!(projects[0]["task_count"], 1);
    assert_eq!(projects[0]["status"], "valid");
}

#[tokio::test]
async fn test_import_preview_malformed_json() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/settings/import/preview"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .body("not valid json{{{")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_import_preview_unknown_fields_ignored() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let json = make_export_json_with_unknown_fields(99);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/settings/import/preview"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let projects = body["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["source_project_id"], "99");
    assert_eq!(projects[0]["task_count"], 1);
}

#[tokio::test]
async fn test_import_preview_empty_projects() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let json = serde_json::json!({
        "exported_at": "2026-07-26T01:26:49.727Z",
        "projects": []
    })
    .to_string();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/settings/import/preview"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

// ─── Import Execute Tests ───────────────────────────────────────────────────

#[tokio::test]
async fn test_import_execute_requires_auth() {
    let (state, auth_layer, _api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/settings/import/execute"))
        .json(&serde_json::json!({
            "raw_json": "{}",
            "imports": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
