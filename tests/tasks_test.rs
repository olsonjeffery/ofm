use std::env;
use std::sync::{Arc, Mutex};

use omprint::db;
use omprint::server;
use omprint::server::state::AppState;

use tempfile::TempDir;
use uuid::Uuid;

struct TestApp {
    addr: String,
    _handle: tokio::task::JoinHandle<()>,
    db: Arc<Mutex<rusqlite::Connection>>,
    project_id: Uuid,
    _git_dir: Option<TempDir>,
    _archive_dir: Option<TempDir>,
}

async fn setup_app() -> TestApp {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    db::run_migrations(&conn).unwrap();
    let user_id = db::ensure_default_user(&conn).unwrap();

    let db = Arc::new(Mutex::new(conn));
    let state = AppState {
        db: db.clone(),
        default_user_id: user_id,
    };

    let project_id = {
        let conn = db.lock().unwrap();
        omprint::services::projects::create_project(
            &conn,
            &user_id,
            "test-project",
            "/tmp/test-repo",
            None,
        )
        .unwrap()
        .id
    };

    let app = server::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestApp {
        addr,
        _handle: handle,
        db,
        project_id,
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
    env::set_var(
        "OMPRINT_ARCHIVE_ROOT",
        archive_dir.path().to_string_lossy().as_ref(),
    );

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    db::run_migrations(&conn).unwrap();
    let user_id = db::ensure_default_user(&conn).unwrap();

    let db = Arc::new(Mutex::new(conn));
    let state = AppState {
        db: db.clone(),
        default_user_id: user_id,
    };

    let project_id = {
        let conn = db.lock().unwrap();
        omprint::services::projects::create_project(
            &conn,
            &user_id,
            "test-project",
            &git_path,
            None,
        )
        .unwrap()
        .id
    };

    let app = server::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestApp {
        addr,
        _handle: handle,
        db,
        project_id,
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
        body["project_id"].as_str().unwrap(),
        app.project_id.to_string()
    );
    assert!(body["id"].as_str().unwrap().len() > 0);

    let task_uuid = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();

    let worktree = {
        let conn = app.db.lock().unwrap();
        omprint::services::tasks::get_worktree_by_task(&conn, &task_uuid).unwrap()
    };

    assert!(std::path::Path::new(&worktree.worktree_path).exists());

    let doc_path = omprint::archive::paths::get_task_doc_path(
        &worktree.project_id.to_string(),
        &worktree.task_id.to_string(),
    )
    .unwrap();
    assert!(doc_path.exists());
    let doc_content = std::fs::read_to_string(&doc_path).unwrap();
    assert_eq!(doc_content, "Implement feature X");
}

#[tokio::test]
async fn test_create_task_missing_project() {
    let app = setup_app().await;
    let fake_id = Uuid::new_v4();
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

    let count: i32 = {
        let conn = app.db.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .unwrap()
    };
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
    let task_id = created["id"].as_str().unwrap().to_string();

    let resp = client()
        .get(format!("{}/api/tasks/{}", app.addr, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], task_id);
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
        .get(format!(
            "{}/api/tasks/00000000-0000-0000-0000-000000000000",
            app.addr
        ))
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
    let task_id = created["id"].as_str().unwrap().to_string();

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
    let task_id = created["id"].as_str().unwrap().to_string();

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
        .put(format!(
            "{}/api/tasks/00000000-0000-0000-0000-000000000000",
            app.addr
        ))
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
    let task_id = created["id"].as_str().unwrap().to_string();
    let task_uuid = Uuid::parse_str(&task_id).unwrap();

    let worktree_path;
    let int_proj;
    let int_task;
    {
        let conn = app.db.lock().unwrap();
        let w = omprint::services::tasks::get_worktree_by_task(&conn, &task_uuid).unwrap();
        worktree_path = w.worktree_path.clone();
        int_proj = w.project_id;
        int_task = w.task_id;
    }

    assert!(
        std::path::Path::new(&worktree_path).exists(),
        "worktree should exist before delete"
    );

    let archive_path =
        omprint::archive::paths::get_task_doc_path(&int_proj.to_string(), &int_task.to_string())
            .unwrap();
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
        let conn = app.db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE id = ?1",
            [task_id.clone()],
            |row| row.get::<_, i32>(0),
        )
        .map(|count| count > 0)
        .unwrap()
    };
    assert!(!task_exists, "task row should be deleted");

    let worktree_exists: bool = {
        let conn = app.db.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM worktrees WHERE task_uuid = ?1",
            [task_id],
            |row| row.get::<_, i32>(0),
        )
        .map(|count| count > 0)
        .unwrap()
    };
    assert!(!worktree_exists, "worktree row should be deleted");
}

#[tokio::test]
async fn test_delete_task_not_found() {
    let app = setup_app().await;
    let resp = client()
        .delete(format!(
            "{}/api/tasks/00000000-0000-0000-0000-000000000000",
            app.addr
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}
