use ofm::auth::AuthLayer;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;

use hiqlite::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

struct TestApp {
    addr: String,
    _handle: tokio::task::JoinHandle<()>,
    db: Client,
    project_id: i64,
    archive_root: String,
    _db_dir: TempDir,
    _git_dir: Option<TempDir>,
    _archive_dir: Option<TempDir>,
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
        archive_root: "storage/".into(),
        _db_dir: db_dir,
        _git_dir: None,
        _archive_dir: None,
    }
}

async fn setup_app_with_git() -> TestApp {
    let git_dir = TempDir::new().unwrap();
    let git_path = git_dir.path().to_string_lossy().to_string();

    let output = tokio::process::Command::new("git")
        .args(["init", &git_path])
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "git init failed");

    let output = tokio::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&git_path)
        .output()
        .await
        .unwrap();
    assert!(output.status.success());

    let output = tokio::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&git_path)
        .output()
        .await
        .unwrap();
    assert!(output.status.success());

    let output = tokio::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&git_path)
        .output()
        .await
        .unwrap();
    assert!(output.status.success(), "git commit failed");

    let archive_dir = TempDir::new().unwrap();
    let archive_root = archive_dir.path().to_string_lossy().to_string();

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

    let project_id =
        ofm::services::projects::create_project(&client, &user_id, "test-project", &git_path, None)
            .await
            .unwrap()
            .id;

    let app_archive_root = archive_root.clone();
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
        archive_root: app_archive_root,
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
        archive_root,
        _db_dir: db_dir,
        _git_dir: Some(git_dir),
        _archive_dir: Some(archive_dir),
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn test_create_task() {
    let app = setup_app_with_git().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "test task",
            "status": "pending",
            "original_request": "Implement feature X",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "test task");
    assert_eq!(body["status"], "pending");
    assert_eq!(
        body["project_id"].as_i64().unwrap(),
        app.project_id
    );
    assert!(body["id"].as_i64().unwrap() > 0);

    let task_id = body["id"].as_i64().unwrap();

    let worktree = ofm::services::tasks::get_worktree_by_task(&app.db, task_id)
        .await
        .unwrap();

    assert!(std::path::Path::new(&worktree.worktree_path).exists());

    let doc_path = std::path::PathBuf::from(&app.archive_root)
        .join("projects")
        .join(worktree.project_id.to_string())
        .join("tasks")
        .join(format!("task-{}.md", worktree.task_id));
    assert!(doc_path.exists());
    let doc_content = std::fs::read_to_string(&doc_path).unwrap();
    assert_eq!(doc_content, "Implement feature X");
}

#[tokio::test]
async fn test_create_task_missing_project() {
    let app = setup_app().await;
    let fake_id: i64 = 99999;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": fake_id,
            "title": "test task",
            "original_request": "some request",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_create_task_empty_title() {
    let app = setup_app().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "",
            "original_request": "some request",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "title is required");
}

#[tokio::test]
async fn test_create_task_rollback_on_worktree_failure() {
    let app = setup_app().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "will fail",
            "original_request": "some request",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 500);

    let mut rows = app
        .db
        .query_raw("SELECT COUNT(*) as cnt FROM tasks", hiqlite::params!())
        .await
        .unwrap();
    let count: i64 = rows.first_mut().map(|r| r.get("cnt")).unwrap_or(0);
    assert_eq!(count, 0, "task row should have been rolled back");
}

#[tokio::test]
async fn test_list_tasks() {
    let app = setup_app_with_git().await;

    for title in &["task B", "task A"] {
        let resp = client()
            .post(format!("{}/api/tasks", app.addr))
            .json(&serde_json::json!({
                "project_id": app.project_id,
                "title": title,
                "original_request": "req",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
    }

    let resp = client()
        .get(format!(
            "{}/api/tasks?project_id={}",
            app.addr, app.project_id
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(body.len(), 2);

    let t1 = body[0]["created_at"].as_str().unwrap().to_string();
    let t2 = body[1]["created_at"].as_str().unwrap().to_string();
    assert!(t1 >= t2, "tasks should be ordered by created_at DESC");
}

#[tokio::test]
async fn test_get_task() {
    let app = setup_app_with_git().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "get-me",
            "original_request": "Test doc content for get_task",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let created: serde_json::Value = resp.json().await.unwrap();
    let task_id: i64 = created["id"].as_i64().unwrap();

    let resp = client()
        .get(format!("{}/api/tasks/{}", app.addr, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_i64().unwrap(), task_id);
    assert_eq!(body["title"], "get-me");

    let doc_content = body["doc_content"].as_str().unwrap();
    assert!(doc_content.contains("Test doc content for get_task"));

    let context_prompt = body["context_prompt"].as_str().unwrap();
    assert!(context_prompt.contains("Task Plan File"));
    assert!(context_prompt.contains("Testing Configuration"));
}

#[tokio::test]
async fn test_get_task_not_found() {
    let app = setup_app().await;
    let resp = client()
        .get(format!("{}/api/tasks/{}", app.addr, 99999))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_update_task() {
    let app = setup_app_with_git().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "original title",
            "original_request": "req",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let created: serde_json::Value = resp.json().await.unwrap();
    let task_id: i64 = created["id"].as_i64().unwrap();

    let resp = client()
        .put(format!("{}/api/tasks/{}", app.addr, task_id))
        .json(&serde_json::json!({
            "title": "updated title",
            "status": "in_progress",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "updated title");
    assert_eq!(body["status"], "in_progress");

    let resp = client()
        .get(format!("{}/api/tasks/{}", app.addr, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "updated title");
    assert_eq!(body["status"], "in_progress");
}

#[tokio::test]
async fn test_update_task_invalid_status() {
    let app = setup_app_with_git().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "status test",
            "original_request": "req",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let created: serde_json::Value = resp.json().await.unwrap();
    let task_id: i64 = created["id"].as_i64().unwrap();

    let resp = client()
        .put(format!("{}/api/tasks/{}", app.addr, task_id))
        .json(&serde_json::json!({
            "status": "bogus",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_update_task_nonexistent() {
    let app = setup_app().await;
    let resp = client()
        .put(format!("{}/api/tasks/{}", app.addr, 99999))
        .json(&serde_json::json!({
            "title": "nope",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_delete_task() {
    let app = setup_app_with_git().await;
    let resp = client()
        .post(format!("{}/api/tasks", app.addr))
        .json(&serde_json::json!({
            "project_id": app.project_id,
            "title": "to-delete",
            "original_request": "delete me",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let created: serde_json::Value = resp.json().await.unwrap();
    let task_id: i64 = created["id"].as_i64().unwrap();

    let worktree_path;
    let int_proj;
    let int_task;
    {
        let w = ofm::services::tasks::get_worktree_by_task(&app.db, task_id)
            .await
            .unwrap();
        worktree_path = w.worktree_path.clone();
        int_proj = w.project_id;
        int_task = w.task_id;
    }

    assert!(
        std::path::Path::new(&worktree_path).exists(),
        "worktree should exist before delete"
    );

    let archive_path = std::path::PathBuf::from(&app.archive_root)
        .join("projects")
        .join(int_proj.to_string())
        .join("tasks")
        .join(format!("task-{}.md", int_task));
    assert!(
        archive_path.exists(),
        "archive doc should exist before delete"
    );

    let resp = client()
        .delete(format!("{}/api/tasks/{}", app.addr, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 204);

    assert!(
        !std::path::Path::new(&worktree_path).exists(),
        "worktree should be removed"
    );

    assert!(!archive_path.exists(), "archive doc should be deleted");

    let task_exists: bool = {
        let mut rows = app
            .db
            .query_raw(
                "SELECT COUNT(*) as cnt FROM tasks WHERE id = $1",
                hiqlite::params!(task_id),
            )
            .await
            .unwrap();
        let count: i64 = rows.first_mut().map(|r| r.get("cnt")).unwrap_or(0);
        count > 0
    };
    assert!(!task_exists, "task row should be deleted");

    let worktree_exists: bool = {
        let mut rows = app
            .db
            .query_raw(
                "SELECT COUNT(*) as cnt FROM worktrees WHERE task_id = $1",
                hiqlite::params!(task_id),
            )
            .await
            .unwrap();
        let count: i64 = rows.first_mut().map(|r| r.get("cnt")).unwrap_or(0);
        count > 0
    };
    assert!(!worktree_exists, "worktree row should be deleted");
}

#[tokio::test]
async fn test_delete_task_not_found() {
    let app = setup_app().await;
    let resp = client()
        .delete(format!("{}/api/tasks/{}", app.addr, 99999))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}
